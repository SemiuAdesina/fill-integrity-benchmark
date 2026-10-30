#![forbid(unsafe_code)]

pub mod engine;
pub mod ports;
pub mod types;

pub use engine::{Engine, EngineConfig};
pub use ports::{
    BlockFill, Clock, FillBatch, FillEvent, FillEventSource, InventorySource, OpenOrder,
    OrderAccepted, OrderGateway, OrderKind, OrderRequest, PortError, StateStore, StrategySource,
    SubmitError,
};
pub use types::{
    BlockRef, ClientRef, Cursor, Duration, InstrumentId, OrderId, PendingOrder, Position, Price,
    Qty, Snapshot, StrategyCandidate, StrategyId, Timestamp, Venue, ATTRIBUTION_WINDOW,
};
