use europa_core::SourceLanguage;

#[test]
fn from_name_maps_python_names_to_python() {
    for name in [
        "python", "Python", "py", "PY", "python3", "ipython", "IPython", " python ",
    ] {
        assert_eq!(
            SourceLanguage::from_name(name),
            SourceLanguage::Python,
            "{name:?}"
        );
    }
}

#[test]
fn from_name_maps_other_names_to_plain() {
    for name in ["julia", "bash", "pythonic", ""] {
        assert_eq!(
            SourceLanguage::from_name(name),
            SourceLanguage::Plain,
            "{name:?}"
        );
    }
}

fn notebook(metadata: &str) -> europa_core::Notebook {
    let json = format!(r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{metadata},"cells":[]}}"#);
    europa_core::parse(json.as_bytes()).expect("notebook loads")
}

#[test]
fn a_notebook_without_metadata_is_python() {
    assert_eq!(notebook("{}").source_language(), SourceLanguage::Python);
}

#[test]
fn language_info_wins_over_kernelspec() {
    let metadata = r#"{"language_info":{"name":"julia"},"kernelspec":{"language":"python"}}"#;
    assert_eq!(notebook(metadata).source_language(), SourceLanguage::Plain);
}

#[test]
fn kernelspec_is_read_without_language_info() {
    let metadata = r#"{"kernelspec":{"language":"julia"}}"#;
    assert_eq!(notebook(metadata).source_language(), SourceLanguage::Plain);
}

#[test]
fn a_name_that_is_not_a_string_counts_as_missing() {
    let metadata = r#"{"language_info":{"name":3},"kernelspec":{"language":"julia"}}"#;
    assert_eq!(notebook(metadata).source_language(), SourceLanguage::Plain);
}

#[test]
fn asking_for_the_language_does_not_change_the_notebook() {
    let metadata = r#"{"language_info":{"name":"julia"},"kernelspec":{"language":"python"}}"#;
    let notebook = notebook(metadata);
    let before = europa_core::serialize(&notebook);
    let _ = notebook.source_language();
    assert_eq!(europa_core::serialize(&notebook), before);
}
