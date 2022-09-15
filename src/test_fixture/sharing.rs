use crate::field::Field;
use crate::replicated_secret_sharing::ReplicatedSecretSharing;
use rand::Rng;
use rand_core::RngCore;

/// Shares `input` into 3 replicated secret shares using the provided `rng` implementation
pub fn share<F: Field, R: RngCore>(input: F, rng: &mut R) -> [ReplicatedSecretSharing<F>; 3] {
    let x1 = F::from(rng.gen::<u128>());
    let x2 = F::from(rng.gen::<u128>());
    let x3 = input - (x1 + x2);

    [
        ReplicatedSecretSharing::new(x1, x2),
        ReplicatedSecretSharing::new(x2, x3),
        ReplicatedSecretSharing::new(x3, x1),
    ]
}

/// Validates correctness of the secret sharing scheme.
///
/// # Panics
/// Panics if the given input is not a valid replicated secret share.
pub fn validate_and_reconstruct<T: Field>(
    input: (
        ReplicatedSecretSharing<T>,
        ReplicatedSecretSharing<T>,
        ReplicatedSecretSharing<T>,
    ),
) -> T {
    assert_eq!(
        input.0.as_tuple().0 + input.1.as_tuple().0 + input.2.as_tuple().0,
        input.0.as_tuple().1 + input.1.as_tuple().1 + input.2.as_tuple().1
    );

    input.0.as_tuple().0 + input.1.as_tuple().0 + input.2.as_tuple().0
}
