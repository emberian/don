use don_sim::systems::combat::cities_capture_local_award_notification::{
    CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_END, CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_SIZE,
    CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_START,
};
use don_sim::systems::combat::cities_capture_plunder_award::{
    CITIES_CAPTURE_PLUNDER_AWARD_END, CITIES_CAPTURE_PLUNDER_AWARD_SIZE,
    CITIES_CAPTURE_PLUNDER_AWARD_START,
};
use don_sim::systems::combat::cities_capture_plunder_gate::{
    CITIES_CAPTURE_PLUNDER_GATE_END, CITIES_CAPTURE_PLUNDER_GATE_SIZE,
    CITIES_CAPTURE_PLUNDER_GATE_START,
};
use don_sim::systems::combat::cities_capture_prefix::{
    CITIES_CAPTURE_CITY_START, CITIES_CAPTURE_PREFIX_END, CITIES_CAPTURE_PREFIX_SIZE,
};
use don_sim::systems::combat::cities_capture_residual::{
    CITIES_CAPTURE_RESIDUAL_END, CITIES_CAPTURE_RESIDUAL_SIZE, CITIES_CAPTURE_RESIDUAL_START,
};
use don_sim::systems::combat::cities_capture_swap_fork::{
    CITIES_CAPTURE_SWAP_FORK_END, CITIES_CAPTURE_SWAP_FORK_SIZE, CITIES_CAPTURE_SWAP_FORK_START,
};
use don_sim::systems::combat::cities_capture_transaction::{
    apply_cities_capture_transaction, CitiesCaptureTransactionError, CitiesCaptureTransactionInput,
    CitiesCaptureTransactionReceipt, CitiesCaptureTransactionWorld, CITIES_CAPTURE_TRANSACTION_END,
    CITIES_CAPTURE_TRANSACTION_SIZE, CITIES_CAPTURE_TRANSACTION_START,
};

// Compile the complete public type identity from outside the crate. The six original
// tranche tests path-imported private copies and therefore could not prove this join.
#[allow(dead_code)]
fn public_transaction_is_callable<W: CitiesCaptureTransactionWorld>(
    world: &mut W,
    input: CitiesCaptureTransactionInput,
) -> Result<CitiesCaptureTransactionReceipt, CitiesCaptureTransactionError> {
    apply_cities_capture_transaction(input, world)
}

#[test]
fn registered_public_tranches_are_one_contiguous_pdb_procedure() {
    assert_eq!(CITIES_CAPTURE_TRANSACTION_START, CITIES_CAPTURE_CITY_START);
    assert_eq!(CITIES_CAPTURE_PREFIX_END, CITIES_CAPTURE_SWAP_FORK_START);
    assert_eq!(
        CITIES_CAPTURE_SWAP_FORK_END,
        CITIES_CAPTURE_PLUNDER_GATE_START
    );
    assert_eq!(
        CITIES_CAPTURE_PLUNDER_GATE_END,
        CITIES_CAPTURE_PLUNDER_AWARD_START
    );
    assert_eq!(
        CITIES_CAPTURE_PLUNDER_AWARD_END,
        CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_START
    );
    assert_eq!(
        CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_END,
        CITIES_CAPTURE_RESIDUAL_START
    );
    assert_eq!(CITIES_CAPTURE_RESIDUAL_END, CITIES_CAPTURE_TRANSACTION_END);
    assert_eq!(
        CITIES_CAPTURE_PREFIX_SIZE
            + CITIES_CAPTURE_SWAP_FORK_SIZE
            + CITIES_CAPTURE_PLUNDER_GATE_SIZE
            + CITIES_CAPTURE_PLUNDER_AWARD_SIZE
            + CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_SIZE
            + CITIES_CAPTURE_RESIDUAL_SIZE,
        CITIES_CAPTURE_TRANSACTION_SIZE
    );
    assert_eq!(CITIES_CAPTURE_TRANSACTION_SIZE, 7_998);
}
