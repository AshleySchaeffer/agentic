mod common;

#[tokio::test]
async fn smoke_daemon_starts_and_responds_to_ping() {
    let daemon = common::TestDaemon::start().await;
    let client = common::TestClient::new(daemon.socket_path().to_path_buf());
    let output = client.ping().await;
    assert_eq!(output.exit_code, 0);
}
