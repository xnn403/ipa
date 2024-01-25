use std::convert::Infallible;

use curve25519_dalek::scalar::Scalar;
use generic_array::GenericArray;
use typenum::{U2, U32};

use crate::{
    ff::{boolean_array::BA256, Field, Serializable},
    impl_shared_value_common,
    protocol::prss::FromRandom,
    secret_sharing::{Block, FieldVectorizable, SharedValue, StdArray, Vectorizable},
};

impl Block for Scalar {
    type Size = U32;
}

///implements the Scalar field for elliptic curve 25519
/// we use elements in Fp25519 to generate curve points and operate on the curve
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fp25519(<Self as SharedValue>::Storage);

impl Fp25519 {
    pub const ONE: Self = Self(Scalar::ONE);

    ///allow invert for scalars, i.e. computes 1/a mod p
    ///# Panics
    /// Panics when self is zero
    #[must_use]
    pub fn invert(&self) -> Fp25519 {
        assert_ne!(*self, Fp25519::ZERO);
        Fp25519(self.0.invert())
    }
}

///trait for secret sharing
impl SharedValue for Fp25519 {
    type Storage = Scalar;
    const BITS: u32 = 256;
    const ZERO: Self = Self(Scalar::ZERO);

    impl_shared_value_common!();
}

///conversion to Scalar struct of `curve25519_dalek`
impl From<Fp25519> for Scalar {
    fn from(s: Fp25519) -> Self {
        s.0
    }
}

impl Serializable for Fp25519 {
    type Size = <<Fp25519 as SharedValue>::Storage as Block>::Size;
    type DeserializationError = Infallible;

    fn serialize(&self, buf: &mut GenericArray<u8, Self::Size>) {
        *buf.as_mut() = self.0.to_bytes();
    }

    /// Deserialized values are reduced modulo the field order.
    fn deserialize(buf: &GenericArray<u8, Self::Size>) -> Result<Self, Self::DeserializationError> {
        Ok(Fp25519(Scalar::from_bytes_mod_order((*buf).into())))
    }
}

///generate random elements in Fp25519
impl rand::distributions::Distribution<Fp25519> for rand::distributions::Standard {
    fn sample<R: crate::rand::Rng + ?Sized>(&self, rng: &mut R) -> Fp25519 {
        let mut scalar_bytes = [0u8; 32];
        rng.fill_bytes(&mut scalar_bytes);
        Fp25519(Scalar::from_bytes_mod_order(scalar_bytes))
    }
}

impl std::ops::Add for Fp25519 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::AddAssign for Fp25519 {
    #[allow(clippy::assign_op_pattern)]
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl std::ops::Neg for Fp25519 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(self.0.neg())
    }
}

impl std::ops::Sub for Fp25519 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl std::ops::SubAssign for Fp25519 {
    #[allow(clippy::assign_op_pattern)]
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl std::ops::Mul for Fp25519 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self(self.0 * rhs.0)
    }
}

impl std::ops::MulAssign for Fp25519 {
    #[allow(clippy::assign_op_pattern)]
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl From<Scalar> for Fp25519 {
    fn from(s: Scalar) -> Self {
        Fp25519(s)
    }
}

/// Conversion from BA256 into Fp25519
///
/// Values are reduced modulo the field order.
impl From<BA256> for Fp25519 {
    fn from(s: BA256) -> Self {
        let mut buf: GenericArray<u8, U32> = [0u8; 32].into();
        s.serialize(&mut buf);
        // Reduces mod order
        Fp25519::deserialize_infallible(&buf)
    }
}

///conversion from and to unsigned integers, preserving entropy, for testing purposes only
#[cfg(test)]
macro_rules! sc_hash_impl {
    ( $u_type:ty) => {
        impl From<Fp25519> for $u_type {
            fn from(s: Fp25519) -> Self {
                use hkdf::Hkdf;
                use sha2::Sha256;
                let hk = Hkdf::<Sha256>::new(None, s.0.as_bytes());
                let mut okm = <$u_type>::MIN.to_le_bytes();
                //error invalid length from expand only happens when okm is very large
                hk.expand(&[], &mut okm).unwrap();
                <$u_type>::from_le_bytes(okm)
            }
        }

        impl From<$u_type> for Fp25519 {
            fn from(s: $u_type) -> Self {
                use hkdf::Hkdf;
                use sha2::Sha256;

                let hk = Hkdf::<Sha256>::new(None, &s.to_le_bytes());
                let mut okm = [0u8; 32];
                //error invalid length from expand only happens when okm is very large
                hk.expand(&[], &mut okm).unwrap();
                Fp25519::deserialize_infallible(&okm.into())
            }
        }
    };
}

#[cfg(test)]
sc_hash_impl!(u64);

impl Vectorizable<1> for Fp25519 {
    type Array = StdArray<Self, 1>;
}

impl FieldVectorizable<1> for Fp25519 {
    type ArrayAlias = StdArray<Self, 1>;
}

impl Field for Fp25519 {
    const NAME: &'static str = "Fp25519";

    const ONE: Fp25519 = Fp25519::ONE;
}

impl FromRandom for Fp25519 {
    type SourceLength = U2;

    fn from_random(src: GenericArray<u128, Self::SourceLength>) -> Self {
        let mut src_bytes = [0u8; 32];
        src_bytes[0..16].copy_from_slice(&src[0].to_le_bytes());
        src_bytes[16..32].copy_from_slice(&src[1].to_le_bytes());
        // Reduces mod order
        Fp25519::deserialize_infallible(<&GenericArray<u8, U32>>::from(&src_bytes))
    }
}

#[cfg(all(test, unit_test))]
mod test {
    use curve25519_dalek::scalar::Scalar;
    use generic_array::GenericArray;
    use rand::{thread_rng, Rng};
    use typenum::U32;

    use crate::{
        ff::{ec_prime_field::Fp25519, Serializable},
        secret_sharing::SharedValue,
    };

    sc_hash_impl!(u32);

    ///test serialize and deserialize
    #[test]
    fn serde_25519() {
        let mut rng = thread_rng();
        let input = rng.gen::<Fp25519>();
        let mut a: GenericArray<u8, U32> = [0u8; 32].into();
        input.serialize(&mut a);
        let output = Fp25519::deserialize_infallible(&a);
        assert_eq!(input, output);
    }

    ///test simple arithmetics to check that `curve25519_dalek` is used correctly
    #[test]
    fn simple_arithmetics_25519() {
        let a = Fp25519(Scalar::from_bytes_mod_order([
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ]));
        let b = Fp25519(Scalar::from_bytes_mod_order([
            0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ]));
        let d = Fp25519(Scalar::from_bytes_mod_order([
            0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ]));
        let e = Fp25519(Scalar::from_bytes_mod_order([
            0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ]));
        let cc = b - a;
        let dc = a + b;
        let ec = a * b;
        assert_eq!(cc, Fp25519::ONE);
        assert_eq!(dc, d);
        assert_eq!(ec, e);
    }

    ///test random field element generation (!= 0)
    #[test]
    fn simple_random_25519() {
        let mut rng = thread_rng();
        assert_ne!(Fp25519::ZERO, rng.gen::<Fp25519>());
    }

    ///test inversion for field elements
    #[test]
    fn invert_25519() {
        let mut rng = thread_rng();
        let a = rng.gen::<Fp25519>();
        let ia = a.invert();
        assert_eq!(a * ia, Fp25519(Scalar::ONE));
    }
}
