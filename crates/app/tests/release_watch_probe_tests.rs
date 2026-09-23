//! Port of `Tests/DownrightAppTests/ReleaseWatchProbeTests.swift`.
//!
//! The production probe's conditional-GET contract: validators round-trip as
//! the matching conditional header, the server's own validators are preferred
//! over ours, and an unchanged feed answers with no body to hash.
//!
//! As in Swift, the session is an ephemeral one whose only protocol class is
//! a stub `NSURLProtocol`, so nothing reaches the network.

mod updater_support;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, NSObjectProtocol};
use objc2::{AllocAnyThread, ClassType, Message, define_class};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSHTTPURLResponse, NSString, NSURLCacheStoragePolicy,
    NSURLProtocol, NSURLProtocolClient, NSURLRequest, NSURLSession, NSURLSessionConfiguration,
};
use upleft_app::updater::release_watch::{ReleaseFeedProbe, ReleaseFeedProbeResult, ReleaseFeedURLProbe};
use upleft_app::updater::update_metadata::Url;
use updater_support::{Skipped, Test, pump};

/// What the stub answers: status, header fields, body.
struct StubResponse {
    status: isize,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

/// The conditional headers of one request the stub saw.
#[derive(Clone, Debug, Default, PartialEq)]
struct SeenRequest {
    if_none_match: Option<String>,
    if_modified_since: Option<String>,
}

static HANDLER: Mutex<Option<StubResponse>> = Mutex::new(None);
static SEEN_REQUESTS: Mutex<Vec<SeenRequest>> = Mutex::new(Vec::new());

fn set_handler(response: StubResponse) {
    *HANDLER.lock().unwrap() = Some(response);
}

fn header(request: &NSURLRequest, field: &str) -> Option<String> {
    request.valueForHTTPHeaderField(&NSString::from_str(field)).map(|value| value.to_string())
}

define_class!(
    // SAFETY: NSURLProtocol subclasses override the four methods below; the
    // class keeps no state of its own.
    #[unsafe(super(NSURLProtocol))]
    #[thread_kind = AllocAnyThread]
    #[name = "UpleftUpdaterTestsStubURLProtocol"]
    struct StubURLProtocol;

    unsafe impl NSObjectProtocol for StubURLProtocol {}

    impl StubURLProtocol {
        #[unsafe(method(canInitWithRequest:))]
        fn can_init_with_request(_request: &NSURLRequest) -> bool {
            true
        }

        #[unsafe(method(canonicalRequestForRequest:))]
        fn canonical_request_for_request(request: &NSURLRequest) -> *mut NSURLRequest {
            Retained::autorelease_return(request.retain())
        }

        #[unsafe(method(startLoading))]
        fn start_loading(&self) {
            let request = self.request();
            SEEN_REQUESTS.lock().unwrap().push(SeenRequest {
                if_none_match: header(&request, "If-None-Match"),
                if_modified_since: header(&request, "If-Modified-Since"),
            });
            let Some(client) = self.client() else { return };
            let handler = HANDLER.lock().unwrap();
            let Some(stub) = handler.as_ref() else {
                let error = NSError::new(-1011, &NSString::from_str("NSURLErrorDomain")); // URLError(.badServerResponse)
                client.URLProtocol_didFailWithError(self, &error);
                return;
            };
            let keys: Vec<Retained<NSString>> = stub.headers.iter().map(|(key, _)| NSString::from_str(key)).collect();
            let values: Vec<Retained<NSString>> = stub.headers.iter().map(|(_, value)| NSString::from_str(value)).collect();
            let fields = NSDictionary::from_retained_objects(&keys.iter().map(|key| &**key).collect::<Vec<_>>(), &values);
            let url = request.URL().expect("a request URL");
            let response = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                NSHTTPURLResponse::alloc(),
                &url,
                stub.status,
                Some(&NSString::from_str("HTTP/1.1")),
                Some(&fields),
            )
            .expect("an HTTP response");
            client.URLProtocol_didReceiveResponse_cacheStoragePolicy(self, &response, NSURLCacheStoragePolicy::NotAllowed);
            client.URLProtocol_didLoadData(self, &NSData::with_bytes(&stub.body));
            client.URLProtocolDidFinishLoading(self);
        }

        #[unsafe(method(stopLoading))]
        fn stop_loading(&self) {}
    }
);

fn make_probe() -> ReleaseFeedURLProbe {
    let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    let classes: Retained<NSArray<AnyClass>> = NSArray::from_slice(&[StubURLProtocol::class()]);
    // SAFETY: an array of `NSURLProtocol` subclasses.
    unsafe { configuration.setProtocolClasses(Some(&classes)) };
    SEEN_REQUESTS.lock().unwrap().clear();
    ReleaseFeedURLProbe::with_session(NSURLSession::sessionWithConfiguration(&configuration))
}

/// `await probe.probe(feed:validator:)`: runs the main run loop until the
/// answer arrives (it is delivered on the main queue).
fn probe_and_wait(probe: &ReleaseFeedURLProbe, feed: &Url, validator: Option<&str>) -> ReleaseFeedProbeResult {
    let result: Rc<RefCell<Option<ReleaseFeedProbeResult>>> = Rc::new(RefCell::new(None));
    let sink = result.clone();
    probe.probe(feed, validator, Box::new(move |answer| *sink.borrow_mut() = Some(answer)));
    assert!(pump(|| result.borrow().is_some(), Duration::from_secs(10)), "the probe never answered");
    result.borrow_mut().take().unwrap()
}

/// "A Last-Modified validator is stored and sent back as If-Modified-Since"
fn last_modified_round_trips() {
    let feed = Url::from_string("https://feeds.example/appcast.xml").unwrap();
    let date = "Wed, 21 Oct 2026 07:28:00 GMT";
    set_handler(StubResponse { status: 200, headers: vec![("Last-Modified", date.into())], body: b"feed".to_vec() });
    let probe = make_probe();

    let ReleaseFeedProbeResult::Changed { validator: Some(validator) } = probe_and_wait(&probe, &feed, None) else {
        panic!("a first probe must report a change");
    };
    assert_eq!(validator, format!("lastModified:{date}"));

    // The next probe carries the date and a 304 answers without a body.
    set_handler(StubResponse { status: 304, headers: vec![], body: Vec::new() });
    let second = probe_and_wait(&probe, &feed, Some(&validator));
    assert_eq!(SEEN_REQUESTS.lock().unwrap().last().unwrap().if_modified_since.as_deref(), Some(date));
    assert_eq!(second, ReleaseFeedProbeResult::Unchanged);
}

/// "The server ETag is preferred over the document date"
fn etag_wins_over_last_modified() {
    let feed = Url::from_string("https://feeds.example/appcast.xml").unwrap();
    set_handler(StubResponse {
        status: 200,
        headers: vec![("ETag", "\"v2\"".into()), ("Last-Modified", "Wed, 21 Oct 2026 07:28:00 GMT".into())],
        body: b"feed".to_vec(),
    });
    let probe = make_probe();

    let ReleaseFeedProbeResult::Changed { validator: Some(validator) } = probe_and_wait(&probe, &feed, None) else {
        panic!("a first probe must report a change");
    };
    assert_eq!(validator, "etag:\"v2\"");
}

/// "A body-hash validator is never offered back as a conditional header"
fn body_hash_stays_local() {
    let feed = Url::from_string("https://feeds.example/appcast.xml").unwrap();
    set_handler(StubResponse { status: 200, headers: vec![], body: b"feed".to_vec() });
    let probe = make_probe();

    let ReleaseFeedProbeResult::Changed { validator: Some(validator) } = probe_and_wait(&probe, &feed, None) else {
        panic!("a first probe must report a change");
    };
    assert!(validator.starts_with("sha256:"));

    set_handler(StubResponse { status: 200, headers: vec![], body: b"feed".to_vec() });
    let second = probe_and_wait(&probe, &feed, Some(&validator));
    let seen = SEEN_REQUESTS.lock().unwrap().last().cloned().unwrap();
    assert_eq!(seen.if_none_match, None);
    assert_eq!(seen.if_modified_since, None);
    assert_eq!(second, ReleaseFeedProbeResult::Unchanged);
}

fn main() {
    updater_support::run(
        "release_watch_probe_tests",
        &[
            Test { name: "ReleaseFeedURLProbeTests/lastModifiedRoundTrips", run: last_modified_round_trips },
            Test { name: "ReleaseFeedURLProbeTests/etagWinsOverLastModified", run: etag_wins_over_last_modified },
            Test { name: "ReleaseFeedURLProbeTests/bodyHashStaysLocal", run: body_hash_stays_local },
        ],
        &[] as &[Skipped],
    );
}
