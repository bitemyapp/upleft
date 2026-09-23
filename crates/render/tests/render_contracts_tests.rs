//! Ports of the render-contract assertions scattered through the Swift
//! suites: `rendererConfigurationIsBoundedAndIndependentOfTheAppLayer`
//! (DecorationTests.swift) and `previewAppearanceContract`
//! (DownrightQLTests/QuickLookPolicyTests.swift), plus the `FragmentPayload`
//! class contract.

use objc2::runtime::AnyObject;
use objc2::{ClassType, Message};
use objc2_app_kit::{NSAppearanceNameAqua, NSAppearanceNameDarkAqua};
use objc2_foundation::{NSArray, NSRange};
use upleft_render::core_types::{BlockIdentity, TableCell, TableData, TableRow};
use upleft_render::render_contracts::{
    FragmentKind, FragmentPayload, MarkdownRenderConfiguration, MarkdownRevealPolicy,
};
use upleft_render::theme::preview_appearance::PreviewAppearance;

#[test]
fn renderer_configuration_is_bounded_and_independent_of_the_app_layer() {
    let mut configuration = MarkdownRenderConfiguration::new(
        true,
        MarkdownRevealPolicy::Never,
        false,
        true,
        true,
        0,
        5,
    );
    assert!(configuration.show_invisibles);
    assert_eq!(configuration.reveal_policy, MarkdownRevealPolicy::Never);
    assert!(configuration.typewriter_scrolling);
    assert_eq!(configuration.code_collapse_threshold(), 1);

    configuration.set_code_collapse_threshold(99_999);
    assert_eq!(configuration.code_collapse_threshold(), 10_000);
}

#[test]
fn preview_appearance_contract() {
    assert_eq!(PreviewAppearance::System.title(), "System");
    assert!(PreviewAppearance::System.ns_appearance().is_none());
    // SAFETY: AppKit exports the appearance names as immutable globals.
    let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let names = NSArray::from_slice(&[aqua, dark]);
    let best = |appearance: PreviewAppearance| {
        appearance
            .ns_appearance()
            .and_then(|ns| ns.bestMatchFromAppearancesWithNames(&names))
    };
    assert_eq!(best(PreviewAppearance::Light).as_deref(), Some(aqua));
    assert_eq!(best(PreviewAppearance::Dark).as_deref(), Some(dark));
    assert_eq!(
        PreviewAppearance::ALL_CASES,
        [
            PreviewAppearance::System,
            PreviewAppearance::Light,
            PreviewAppearance::Dark
        ]
    );
}

#[test]
fn fragment_payload_is_an_objective_c_class_named_like_the_swift_one() {
    assert_eq!(
        FragmentPayload::class().name().to_str().unwrap(),
        "FragmentPayload"
    );
    let payload = FragmentPayload::new(
        FragmentKind::Table,
        NSRange::new(10, 20),
        BlockIdentity {
            kind: 7,
            ordinal: 2,
        },
        "detail",
    );
    // It rides along as an attribute value: an object, retained, not copied.
    let object: &AnyObject = payload.as_ref();
    assert!(object.downcast_ref::<FragmentPayload>().is_some());
    let retained = payload.retain();
    retained.set_is_collapsed(true);
    assert!(payload.is_collapsed());
    assert_eq!(payload.kind(), FragmentKind::Table);
    assert_eq!(payload.detail(), "detail");
    assert_eq!(
        payload.block_identity(),
        BlockIdentity {
            kind: 7,
            ordinal: 2
        }
    );
}

#[test]
fn fragment_payload_projects_its_ranges_across_an_edit() {
    let payload = FragmentPayload::new(
        FragmentKind::Table,
        NSRange::new(10, 20),
        BlockIdentity {
            kind: 1,
            ordinal: 0,
        },
        "",
    );
    payload.set_table_data(Some(TableData {
        rows: vec![TableRow {
            range: NSRange::new(10, 8),
            cells: vec![TableCell {
                range: NSRange::new(11, 3),
                content_range: NSRange::new(12, 1),
                inlines: Vec::new(),
            }],
            is_header: true,
        }],
        alignments: Vec::new(),
        delimiter_range: NSRange::new(19, 5),
    }));
    // Insert three characters before the table: everything shifts right.
    payload.project_source_ranges(NSRange::new(0, 0), 3);
    assert_eq!(payload.source_range(), NSRange::new(13, 20));
    let table = payload.table_data().clone().unwrap();
    assert_eq!(table.delimiter_range, NSRange::new(22, 5));
    assert_eq!(table.rows[0].range, NSRange::new(13, 8));
    assert_eq!(table.rows[0].cells[0].content_range, NSRange::new(15, 1));

    // Replace two characters inside the table with five.
    payload.project_source_ranges(NSRange::new(15, 2), 5);
    assert_eq!(payload.source_range(), NSRange::new(13, 23));
    // A range entirely after the edit moves by the delta.
    assert_eq!(
        payload.table_data().as_ref().unwrap().delimiter_range,
        NSRange::new(25, 5)
    );
}
