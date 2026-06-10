//! Integration test for the embedder user-agent product override on
//! the registry side.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because `set_user_agent_product` is once-per-process and the
//! assembled header is cached in a `OnceLock`: registering a product
//! here must not leak into the unit tests that exercise the default
//! `aube/<version>` identity.

use aube_registry::client::RegistryClient;
use aube_registry::config::NpmConfig;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn registered_product_leads_the_registry_user_agent_header() {
    aube_util::ua::set_user_agent_product("mytool/2.1.0");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/demo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "demo",
            "versions": {},
            "dist-tags": {},
        })))
        .mount(&server)
        .await;

    let client = RegistryClient::from_config(NpmConfig {
        registry: format!("{}/", server.uri()),
        ..Default::default()
    });
    client
        .fetch_packument_json_fresh("demo")
        .await
        .expect("mock packument fetch should succeed");

    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled by default");
    let ua = requests[0]
        .headers
        .get("user-agent")
        .expect("registry requests must carry a User-Agent header")
        .to_str()
        .expect("UA header should be valid UTF-8");
    assert!(
        ua.starts_with("mytool/2.1.0 ("),
        "product token must lead the registry UA, got: {ua}"
    );
    assert!(
        !ua.contains("aube/"),
        "default product must be fully replaced, got: {ua}"
    );
}
