use crate::client::Client;
#[cfg(feature = "worldgen")]
use network::{ConnectAddr, ListenAddr, Network, Participant, Pid, Promises, Stream};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[cfg(feature = "worldgen")]
use tokio::runtime::Runtime;

#[cfg(feature = "worldgen")]
pub(crate) struct ClientSupport {
    _runtime: Arc<Runtime>,
    _server_network: Network,
    _remote_network: Network,
    _remote_participant: Participant,
    _remote_streams: Vec<Stream>,
}

#[cfg(feature = "worldgen")]
fn next_mpsc_addrs() -> (ListenAddr, ConnectAddr) {
    static NEXT_PORT: AtomicU64 = AtomicU64::new(50_000);

    let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    (ListenAddr::Mpsc(port), ConnectAddr::Mpsc(port))
}

#[cfg(feature = "worldgen")]
pub(crate) fn make_test_client() -> (ClientSupport, Client) {
    let runtime = Arc::new(Runtime::new().expect("tokio runtime"));
    let (listen_addr, connect_addr) = next_mpsc_addrs();
    let connect_addr_clone = connect_addr.clone();
    let (server_network, remote_network, participant, remote_participant, remote_streams, streams) =
        runtime.block_on(async {
            let mut server_network = Network::new(Pid::new(), &runtime);
            let remote_network = Network::new(Pid::new(), &runtime);

            server_network
                .listen(listen_addr)
                .await
                .expect("listen on mpsc");
            let mut remote_participant = remote_network
                .connect(connect_addr_clone.clone())
                .await
                .expect("connect remote participant");
            let participant = server_network
                .connected()
                .await
                .expect("accept server-side participant");

            let general_stream = participant
                .open(3, Promises::ORDERED, 500)
                .await
                .expect("open general stream");
            let general_remote = remote_participant.opened().await.expect("opened general");
            let ping_stream = participant
                .open(2, Promises::ORDERED, 500)
                .await
                .expect("open ping stream");
            let ping_remote = remote_participant.opened().await.expect("opened ping");
            let register_stream = participant
                .open(3, Promises::ORDERED, 500)
                .await
                .expect("open register stream");
            let register_remote = remote_participant.opened().await.expect("opened register");
            let character_screen_stream = participant
                .open(3, Promises::ORDERED, 500)
                .await
                .expect("open character screen stream");
            let character_screen_remote = remote_participant
                .opened()
                .await
                .expect("opened character screen");
            let in_game_stream = participant
                .open(3, Promises::ORDERED, 100_000)
                .await
                .expect("open in-game stream");
            let in_game_remote = remote_participant.opened().await.expect("opened in-game");
            let terrain_stream = participant
                .open(4, Promises::ORDERED, 20_000)
                .await
                .expect("open terrain stream");
            let terrain_remote = remote_participant.opened().await.expect("opened terrain");

            (
                server_network,
                remote_network,
                participant,
                remote_participant,
                vec![
                    general_remote,
                    ping_remote,
                    register_remote,
                    character_screen_remote,
                    in_game_remote,
                    terrain_remote,
                ],
                (
                    general_stream,
                    ping_stream,
                    register_stream,
                    character_screen_stream,
                    in_game_stream,
                    terrain_stream,
                ),
            )
        });

    let client = Client::new(
        common_net::msg::ClientType::Game,
        participant,
        connect_addr,
        0.0,
        None,
        streams.0,
        streams.1,
        streams.2,
        streams.3,
        streams.4,
        streams.5,
    );

    (
        ClientSupport {
            _runtime: runtime,
            _server_network: server_network,
            _remote_network: remote_network,
            _remote_participant: remote_participant,
            _remote_streams: remote_streams,
        },
        client,
    )
}
