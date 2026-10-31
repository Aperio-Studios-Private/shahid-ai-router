import io

p = 'crates/sair-gateway/src/lib.rs'
src = io.open(p, encoding='utf-8').read()

src = src.replace('''#[derive(Clone)]
pub struct GatewayRuntimeState {
    expected_token: Arc<tokio::sync::RwLock<Arc<Vec<u8>>>>,
    permits: Arc<tokio::sync::Semaphore>,
}''', '''#[derive(Clone)]
pub struct GatewayRuntimeState {
    expected_token: Arc<tokio::sync::RwLock<Arc<Vec<u8>>>>,
    permits: Arc<tokio::sync::Semaphore>,
    /// Phase 11: live handler count for drain semantics (idle keep-alive
    /// connections are closed at drain; only handlers are awaited).
    pub(crate) in_flight: Arc<std::sync::atomic::AtomicUsize>,
}''')

src = src.replace('''    let state = GatewayRuntimeState {
        expected_token: Arc::new(tokio::sync::RwLock::new(config.token.clone())),
        permits: Arc::new(tokio::sync::Semaphore::new(config.concurrency)),
    };
    router_with_state(config, state)''', '''    let state = GatewayRuntimeState {
        expected_token: Arc::new(tokio::sync::RwLock::new(config.token.clone())),
        permits: Arc::new(tokio::sync::Semaphore::new(config.concurrency)),
        in_flight: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    router_with_state(config, state)''')

src = src.replace('''    if token::verify_token(provided, &expected) {
        next.run(req).await
    } else {''', '''    if token::verify_token(provided, &expected) {
        state
            .in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let response = next.run(req).await;
        state
            .in_flight
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        response
    } else {''')

src = src.replace('''fn openai_error(status: StatusCode, code: &str, message: &str, extra: Value) -> Response {''', '''static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Waits until no handler is executing (drain semantics, FR-017: only
/// handlers are awaited; idle keep-alive connections close at drain).
pub async fn wait_for_in_flight_idle() {
    loop {
        let count = IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst);
        if count == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn openai_error(status: StatusCode, code: &str, message: &str, extra: Value) -> Response {''')

io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('lib.rs in-flight added')

p = 'crates/sair-gateway/src/handle.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''        let state = crate::GatewayRuntimeState {
            expected_token: token.clone(),
            permits: Arc::new(tokio::sync::Semaphore::new(config.concurrency)),
        };''', '''        let state = crate::GatewayRuntimeState {
            expected_token: token.clone(),
            permits: Arc::new(tokio::sync::Semaphore::new(config.concurrency)),
            in_flight: crate::IN_FLIGHT_STATE.clone(),
        };''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('handle state fixed')
