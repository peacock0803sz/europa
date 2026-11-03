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
