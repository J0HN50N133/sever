use common::revocation::{
    blockchain_service_server::{BlockchainService, BlockchainServiceServer},
    AccumulatorState, UpdateAccumulatorRequest,
};
use parking_lot::Mutex;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct MyBlockchain {
    state: Arc<Mutex<BlockchainState>>,
}

#[derive(Debug, Default)]
struct BlockchainState {
    root_hash: Vec<u8>,
    version: u64,
}

impl MyBlockchain {
    pub fn new() -> Self {
        let initial_state = BlockchainState {
            root_hash: vec![0; 32], // Initial empty hash
            version: 0,
        };
        MyBlockchain {
            state: Arc::new(Mutex::new(initial_state)),
        }
    }

    pub fn into_server(self) -> BlockchainServiceServer<MyBlockchain> {
        BlockchainServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl BlockchainService for MyBlockchain {
    async fn get_accumulator(
        &self,
        _request: Request<()>, // Use imported Empty
    ) -> Result<Response<AccumulatorState>, Status> {
        let state = self.state.lock();
        let reply = AccumulatorState {
            root_hash: state.root_hash.clone(),
            version: state.version,
        };
        Ok(Response::new(reply))
    }

    async fn update_accumulator(
        &self,
        request: Request<UpdateAccumulatorRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        let mut state = self.state.lock();
        state.root_hash = req.new_root_hash;
        state.version = req.new_version;
        log::info!(
            "Blockchain updated to version {} with new root hash: {:?}",
            state.version,
            state.root_hash
        );
        Ok(Response::new(())) // Use imported Empty
    }
}
