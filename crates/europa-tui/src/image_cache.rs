//! A cache of decoded images and of what was made of them for the terminal.

use std::sync::Arc;

use europa_core::{CellPixels, DecodedImage};
use ratatui_image::picker::ProtocolType;
use ratatui_image::sliced::SlicedProtocol;

/// Default limit of the cache, in bytes.
pub(crate) const DEFAULT_LIMIT: usize = 256 * 1024 * 1024;

/// What an encoded image was made for.
///
/// An encoding is only used again when all of it is equal, because a different
/// slot size, cell size or protocol needs different data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncodeKey {
    /// Columns of the slot.
    pub(crate) columns: u16,
    /// Lines of the slot.
    pub(crate) rows: u16,
    /// Size of a cell in pixels.
    pub(crate) cell_pixels: CellPixels,
    /// The protocol the data is for.
    pub(crate) protocol: ProtocolType,
}

/// The outcome of encoding an image for a slot.
pub(crate) enum Encoded {
    /// Data ready to be drawn.
    Ready(SlicedProtocol),
    /// Encoding failed, and is not tried again under the same key.
    Failed,
}

/// One image and what is kept with it.
struct Entry {
    source: u64,
    pixels: Arc<DecodedImage>,
    /// The last encoding, and what it was made for.
    encoded: Option<(EncodeKey, Encoded)>,
    /// Set once drawing with the chosen protocol failed on screen.
    forced_halfblock: bool,
    /// The value of the clock when the entry was last used.
    last_used: u64,
}

impl Entry {
    fn bytes(&self) -> usize {
        let encoded = match &self.encoded {
            Some((key, Encoded::Ready(_))) => {
                // Kitty sends raw RGBA as base64, which is about 5.3 bytes a
                // pixel, and other protocols are of the same order.
                usize::from(key.columns)
                    * usize::from(key.cell_pixels.width.get())
                    * usize::from(key.rows)
                    * usize::from(key.cell_pixels.height.get())
                    * 6
            }
            _ => 0,
        };
        self.pixels.rgb.len() + encoded
    }
}

/// A cache with a limit on its total size, that drops what was used longest
/// ago first.
///
/// There are few images at a time, so entries sit in a `Vec` that is searched
/// in order instead of in a map with an order of use kept on the side.
pub(crate) struct ImageCache {
    limit: usize,
    entries: Vec<Entry>,
    clock: u64,
}

impl ImageCache {
    /// A cache that holds at most `limit` bytes, with one image over that
    /// limit as the exception.
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            entries: Vec::new(),
            clock: 0,
        }
    }

    /// Adds an image, or only marks it as used when it is already there, so
    /// that its encoding stays.
    pub(crate) fn insert(&mut self, source: u64, pixels: DecodedImage) {
        if self.touch(source).is_none() {
            self.clock += 1;
            self.entries.push(Entry {
                source,
                pixels: Arc::new(pixels),
                encoded: None,
                forced_halfblock: false,
                last_used: self.clock,
            });
        }
        self.trim();
    }

    /// The pixels of an image, which then count as used.
    pub(crate) fn pixels(&mut self, source: u64) -> Option<Arc<DecodedImage>> {
        self.touch(source).map(|entry| Arc::clone(&entry.pixels))
    }

    /// The encoding of an image when it was made under `key`, which then
    /// counts as used.
    pub(crate) fn encoded(&mut self, source: u64, key: &EncodeKey) -> Option<&Encoded> {
        match &self.touch(source)?.encoded {
            Some((made_for, encoded)) if made_for == key => Some(encoded),
            _ => None,
        }
    }

    /// Replaces the encoding of an image.
    pub(crate) fn store_encoded(&mut self, source: u64, key: EncodeKey, encoded: Encoded) {
        if let Some(entry) = self.touch(source) {
            entry.encoded = Some((key, encoded));
        }
        self.trim();
    }

    /// Drops the encoding of an image and marks it to be drawn with
    /// half-blocks from now on.
    pub(crate) fn force_halfblock(&mut self, source: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.source == source) {
            entry.encoded = None;
            entry.forced_halfblock = true;
        }
    }

    /// Whether the image is marked to be drawn with half-blocks.
    pub(crate) fn is_forced_halfblock(&self, source: u64) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.source == source && entry.forced_halfblock)
    }

    /// The size of everything held, in bytes.
    pub(crate) fn total_bytes(&self) -> usize {
        self.entries.iter().map(Entry::bytes).sum()
    }

    /// Marks an entry as the one used last, and returns it.
    fn touch(&mut self, source: u64) -> Option<&mut Entry> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.source == source)?;
        entry.last_used = clock;
        Some(entry)
    }

    /// Drops the entries used longest ago until the total fits. The entry
    /// used last stays even when it alone is over the limit, because the
    /// image on screen has to be drawn.
    fn trim(&mut self) {
        while self.entries.len() > 1 && self.total_bytes() > self.limit {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(at, _)| at)
                .expect("there is more than one entry");
            self.entries.swap_remove(oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use ratatui_image::protocol::halfblocks::Halfblocks;

    use super::*;

    /// An image of `bytes / 3` pixels, so that it counts as `bytes` bytes.
    fn image(bytes: u32) -> DecodedImage {
        DecodedImage {
            width: bytes / 3,
            height: 1,
            rgb: vec![0; bytes as usize],
        }
    }

    fn key(columns: u16, rows: u16) -> EncodeKey {
        EncodeKey {
            columns,
            rows,
            cell_pixels: CellPixels::DEFAULT,
            protocol: ProtocolType::Halfblocks,
        }
    }

    fn ready() -> Encoded {
        Encoded::Ready(SlicedProtocol::Halfblocks(Halfblocks::default()))
    }

    /// What a ready encoding for `key(columns, rows)` counts as.
    fn ready_bytes(columns: u16, rows: u16) -> usize {
        usize::from(columns) * 10 * usize::from(rows) * 20 * 6
    }

    #[test]
    fn the_total_stays_within_the_limit() {
        let mut cache = ImageCache::new(300);
        for source in 0..10 {
            cache.insert(source, image(90));
            assert!(cache.total_bytes() <= 300);
        }
        assert_eq!(cache.total_bytes(), 270);
    }

    #[test]
    fn the_image_used_longest_ago_goes_first() {
        let mut cache = ImageCache::new(200);
        cache.insert(1, image(90));
        cache.insert(2, image(90));
        cache.insert(3, image(90));
        assert!(cache.pixels(1).is_none());
        assert!(cache.pixels(2).is_some());
        assert!(cache.pixels(3).is_some());
    }

    #[test]
    fn taking_an_image_makes_it_new_again() {
        let mut cache = ImageCache::new(200);
        cache.insert(1, image(90));
        cache.insert(2, image(90));
        assert!(cache.pixels(1).is_some());
        cache.insert(3, image(90));
        assert!(cache.pixels(1).is_some());
        assert!(cache.pixels(2).is_none());
    }

    #[test]
    fn an_encoding_counts_as_use_of_its_image() {
        let mut cache = ImageCache::new(ready_bytes(1, 1) * 2 + 90 * 2);
        cache.insert(1, image(90));
        cache.store_encoded(1, key(1, 1), ready());
        cache.insert(2, image(90));
        cache.store_encoded(2, key(1, 1), ready());
        assert!(cache.encoded(1, &key(1, 1)).is_some());
        cache.insert(3, image(90));
        cache.store_encoded(3, key(1, 1), ready());
        assert!(cache.pixels(1).is_some());
        assert!(cache.pixels(2).is_none());
    }

    #[test]
    fn one_image_over_the_limit_stays_and_pushes_the_others_out() {
        let mut cache = ImageCache::new(100);
        cache.insert(1, image(30));
        cache.insert(2, image(30));
        cache.insert(3, image(3000));
        assert!(cache.pixels(1).is_none());
        assert!(cache.pixels(2).is_none());
        assert!(cache.pixels(3).is_some());
        assert_eq!(cache.total_bytes(), 3000);
    }

    #[test]
    fn inserting_a_source_again_keeps_its_encoding() {
        let mut cache = ImageCache::new(10_000_000);
        cache.insert(1, image(30));
        cache.store_encoded(1, key(2, 1), ready());
        cache.insert(1, image(30));
        assert!(matches!(
            cache.encoded(1, &key(2, 1)),
            Some(Encoded::Ready(_))
        ));
        assert_eq!(cache.total_bytes(), 30 + ready_bytes(2, 1));
    }

    #[test]
    fn an_encoding_is_only_found_under_the_key_it_was_made_for() {
        let mut cache = ImageCache::new(10_000_000);
        cache.insert(1, image(30));
        cache.store_encoded(1, key(2, 1), ready());
        assert!(cache.encoded(1, &key(3, 1)).is_none());
        let other = EncodeKey {
            cell_pixels: CellPixels {
                width: NonZeroU16::new(8).expect("non-zero"),
                height: NonZeroU16::new(16).expect("non-zero"),
            },
            ..key(2, 1)
        };
        assert!(cache.encoded(1, &other).is_none());
        let other = EncodeKey {
            protocol: ProtocolType::Sixel,
            ..key(2, 1)
        };
        assert!(cache.encoded(1, &other).is_none());
        assert!(cache.encoded(1, &key(2, 1)).is_some());
        assert!(cache.encoded(2, &key(2, 1)).is_none());
    }

    #[test]
    fn forcing_half_blocks_drops_the_encoding_and_leaves_a_mark() {
        let mut cache = ImageCache::new(10_000_000);
        cache.insert(1, image(30));
        cache.insert(2, image(30));
        cache.store_encoded(1, key(2, 1), ready());
        assert!(!cache.is_forced_halfblock(1));
        cache.force_halfblock(1);
        assert!(cache.is_forced_halfblock(1));
        assert!(!cache.is_forced_halfblock(2));
        assert!(cache.encoded(1, &key(2, 1)).is_none());
        assert_eq!(cache.total_bytes(), 60);
        assert!(cache.pixels(1).is_some());
    }

    #[test]
    fn a_failed_encoding_counts_for_nothing_and_a_ready_one_for_its_pixels() {
        let mut cache = ImageCache::new(10_000_000);
        cache.insert(1, image(30));
        cache.store_encoded(1, key(4, 3), Encoded::Failed);
        assert_eq!(cache.total_bytes(), 30);
        cache.store_encoded(1, key(4, 3), ready());
        assert_eq!(cache.total_bytes(), 30 + ready_bytes(4, 3));
        assert!(matches!(
            cache.encoded(1, &key(4, 3)),
            Some(Encoded::Ready(_))
        ));
    }

    #[test]
    fn an_image_that_is_not_there_is_left_alone() {
        let mut cache = ImageCache::new(100);
        cache.store_encoded(7, key(1, 1), ready());
        cache.force_halfblock(7);
        assert!(!cache.is_forced_halfblock(7));
        assert_eq!(cache.total_bytes(), 0);
    }
}
