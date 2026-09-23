//! Port of `Tests/DownrightAppTests/ReaderProfileTests.swift`.
//!
//! Not ported: `pickerListsBuiltInsAndSendsLivePreview` and
//! `savingCustomProfileUsesInjectedStore` (`ReaderProfilePickerView`, an
//! AppKit view, ported with the UI).

use upleft_app::support::reader_profiles::{JSONReaderProfileStore, ReaderProfile, ReaderProfileStore};
use upleft_core::contracts::Uuid;
use upleft_foundation::url::FileUrl;

#[test]
fn built_ins_cover_the_reader_jobs() {
    let names: Vec<String> = ReaderProfile::built_ins().into_iter().map(|profile| profile.name).collect();
    assert_eq!(names, ["Documentation", "Long-form", "Academic", "Specification", "GitHub", "Presentation"]);
    assert!(ReaderProfile::built_ins().iter().all(|profile| profile.is_built_in));
}

#[test]
fn profile_clamps_reading_measure() {
    let measure = |value: f64| {
        let base = ReaderProfile::new("x", "x");
        ReaderProfile::with("x", "x", false, base.typography_scale, value, base.chrome_density, base.motion_preference)
            .measure_characters
    };
    assert_eq!(measure(10.0), 68.0);
    assert_eq!(measure(100.0), 72.0);
}

#[test]
fn custom_profiles_round_trip_through_injected_store() {
    // Swift's test names the file "reader-(UUID().uuidString).json" (the
    // interpolation is missing its backslash); any unique name will do.
    let path = std::env::temp_dir().join(format!("reader-{}.json", Uuid::new_v4()));
    let url = FileUrl::from_path(path.to_str().unwrap());
    let store = JSONReaderProfileStore::new(url);
    let profile = ReaderProfile::custom("My setup");
    store.save_custom_profiles(&[profile.clone(), ReaderProfile::built_ins()[0].clone()]);
    assert_eq!(store.load_custom_profiles(), vec![profile]);
    let _ = std::fs::remove_file(&path);
}
