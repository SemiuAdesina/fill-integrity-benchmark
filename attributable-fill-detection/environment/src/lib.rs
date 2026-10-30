#![forbid(unsafe_code)]

pub mod engine;
pub mod ports;
pub mod types;

pub use engine::{Engine, EngineConfig};
pub use ports::{
    InventorySource, Clock, FillBatch, FillEvent, FillEventSource, StrategySource, OpenOrder,
    OrderAccepted, OrderGateway, OrderKind, OrderRequest, PortError, StateStore, SubmitError,
};
pub use types::{
    ClientRef, Cursor, Duration, StrategyCandidate, StrategyId, OrderId, PendingOrder, Position, Price,
    Qty, Venue, Snapshot, Timestamp, InstrumentId,
};
