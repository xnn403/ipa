use crate::{
    bits::{Fp2Array, Serializable},
    error::Error,
    ff::Field,
    helpers::Role,
    protocol::{
        basics::reshare::LegacyReshare, context::Context, sort::apply_sort::shuffle::Resharable,
        RecordId, Substep,
    },
    secret_sharing::{
        replicated::{
            malicious::{
                AdditiveShare as MaliciousReplicated, DowngradeMalicious,
                ThisCodeIsAuthorizedToDowngradeFromMalicious, UnauthorizedDowngradeWrapper,
            },
            semi_honest::{AdditiveShare as Replicated, AdditiveShare, XorShare},
        },
        Arithmetic,
    },
};
use async_trait::async_trait;
use futures::future::{try_join, try_join3};
use generic_array::GenericArray;
use std::marker::PhantomData;
use typenum::Unsigned;

//
// 'apply_attribution_window` protocol
//
#[derive(Debug)]
pub struct ApplyAttributionWindowInputRow<F: Field, BK: Fp2Array> {
    pub timestamp: AdditiveShare<F>,
    pub is_trigger_report: AdditiveShare<F>,
    pub helper_bit: AdditiveShare<F>,
    pub breakdown_key: XorShare<BK>,
    pub trigger_value: AdditiveShare<F>,
}

#[derive(Debug)]
pub struct MCApplyAttributionWindowInputRow<F: Field, T: Arithmetic<F>> {
    pub timestamp: T,
    pub is_trigger_report: T,
    pub helper_bit: T,
    pub breakdown_key: Vec<T>,
    pub trigger_value: T,
    _marker: PhantomData<F>,
}

impl<F: Field, T: Arithmetic<F>> MCApplyAttributionWindowInputRow<F, T> {
    pub fn new(
        timestamp: T,
        is_trigger_report: T,
        helper_bit: T,
        breakdown_key: Vec<T>,
        trigger_value: T,
    ) -> Self {
        Self {
            timestamp,
            is_trigger_report,
            helper_bit,
            breakdown_key,
            trigger_value,
            _marker: PhantomData,
        }
    }
}

pub type MCApplyAttributionWindowOutputRow<F, T> = MCAccumulateCreditInputRow<F, T>;

//
// `accumulate_credit` protocol
//

pub struct AccumulateCreditInputRow<F: Field, BK: Fp2Array> {
    pub is_trigger_report: AdditiveShare<F>,
    pub helper_bit: AdditiveShare<F>,
    pub breakdown_key: XorShare<BK>,
    pub trigger_value: AdditiveShare<F>,
}

#[derive(Debug)]
pub struct MCAccumulateCreditInputRow<F: Field, T: Arithmetic<F>> {
    pub is_trigger_report: T,
    pub helper_bit: T,
    pub breakdown_key: Vec<T>,
    pub trigger_value: T,
    _marker: PhantomData<F>,
}

impl<F: Field, T: Arithmetic<F>> MCAccumulateCreditInputRow<F, T> {
    pub fn new(
        is_trigger_report: T,
        helper_bit: T,
        breakdown_key: Vec<T>,
        trigger_value: T,
    ) -> Self {
        Self {
            is_trigger_report,
            helper_bit,
            breakdown_key,
            trigger_value,
            _marker: PhantomData,
        }
    }
}

pub type MCAccumulateCreditOutputRow<F, T> = MCAccumulateCreditInputRow<F, T>;

//
// `credit_capping` protocol
//
pub type CreditCappingInputRow<F, BK> = AccumulateCreditInputRow<F, BK>;
pub type MCCreditCappingInputRow<F, T> = MCAccumulateCreditInputRow<F, T>;

#[derive(Debug)]
pub struct MCCreditCappingOutputRow<F: Field, T: Arithmetic<F>> {
    pub breakdown_key: Vec<T>,
    pub credit: T,
    _marker: PhantomData<F>,
}
impl<F: Field, T: Arithmetic<F>> MCCreditCappingOutputRow<F, T> {
    pub fn new(breakdown_key: Vec<T>, credit: T) -> Self {
        Self {
            breakdown_key,
            credit,
            _marker: PhantomData,
        }
    }
}

#[async_trait]
impl<F: Field> DowngradeMalicious for MCCappedCreditsWithAggregationBit<F, MaliciousReplicated<F>> {
    type Target = MCCappedCreditsWithAggregationBit<F, Replicated<F>>;
    /// For ShuffledPermutationWrapper on downgrading, we return revealed permutation. This runs reveal on the malicious context
    async fn downgrade(self) -> UnauthorizedDowngradeWrapper<Self::Target> {
        // Note that this clones the values rather than moving them.
        UnauthorizedDowngradeWrapper::new(Self::Target::new(
            self.helper_bit.x().access_without_downgrade().clone(),
            self.aggregation_bit.x().access_without_downgrade().clone(),
            self.breakdown_key
                .into_iter()
                .map(|bk| bk.x().access_without_downgrade().clone())
                .collect::<Vec<_>>(),
            self.credit.x().access_without_downgrade().clone(),
        ))
    }
}

#[async_trait]
impl<F: Field, BK: Fp2Array> DowngradeMalicious
    for MCAggregateCreditOutputRow<F, MaliciousReplicated<F>, BK>
where
    Replicated<F>: Serializable,
{
    type Target = MCAggregateCreditOutputRow<F, Replicated<F>, BK>;
    async fn downgrade(self) -> UnauthorizedDowngradeWrapper<Self::Target> {
        UnauthorizedDowngradeWrapper::new(Self::Target::new(
            self.breakdown_key
                .into_iter()
                .map(|bk| bk.x().access_without_downgrade().clone())
                .collect::<Vec<_>>(),
            self.credit.x().access_without_downgrade().clone(),
        ))
    }
}

//
// `aggregate_credit` protocol
//

#[derive(Debug)]
pub struct AggregateCreditInputRow<F: Field, BK: Fp2Array> {
    pub breakdown_key: XorShare<BK>,
    pub credit: AdditiveShare<F>,
}

pub type MCAggregateCreditInputRow<F, T> = MCCreditCappingOutputRow<F, T>;

#[derive(Debug)]
pub struct MCCappedCreditsWithAggregationBit<F, T> {
    pub helper_bit: T,
    pub aggregation_bit: T,
    pub breakdown_key: Vec<T>,
    pub credit: T,
    marker: PhantomData<F>,
}

impl<F: Field, T: Arithmetic<F>> MCCappedCreditsWithAggregationBit<F, T> {
    pub fn new(helper_bit: T, aggregation_bit: T, breakdown_key: Vec<T>, credit: T) -> Self {
        Self {
            helper_bit,
            aggregation_bit,
            breakdown_key,
            credit,
            marker: PhantomData,
        }
    }
}

#[derive(Debug)]
// TODO: `breakdown_key`'s length == `<BK as BitArray>::BITS`.
//       instead of having a `Vec`, we can probably use an array since the length is known at compile time
pub struct MCAggregateCreditOutputRow<F, T, BK> {
    pub breakdown_key: Vec<T>,
    pub credit: T,
    _marker: PhantomData<(F, BK)>,
}

impl<F: Field, T: Arithmetic<F>, BK: Fp2Array> MCAggregateCreditOutputRow<F, T, BK>
where
    T: Serializable,
{
    /// We know there will be exactly `BK::BITS` number of `breakdown_key` parts
    pub const SIZE: usize = (BK::BITS as usize + 1) * <T as Serializable>::Size::USIZE;

    pub fn new(breakdown_key: Vec<T>, credit: T) -> Self {
        Self {
            breakdown_key,
            credit,
            _marker: PhantomData,
        }
    }

    /// writes the bytes of `MCAggregateCreditOutputRow` into the `buf`.
    /// # Panics
    /// if `breakdown_key` has unexpected length.
    /// if `buf` is not the right length
    pub fn serialize(self, buf: &mut [u8]) {
        assert_eq!(self.breakdown_key.len(), BK::BITS as usize);
        assert_eq!(buf.len(), Self::SIZE);

        let breakdown_key_size = self.breakdown_key.len() * <T as Serializable>::Size::USIZE;

        for (i, key_part) in self.breakdown_key.into_iter().enumerate() {
            key_part.serialize(GenericArray::from_mut_slice(
                &mut buf[<T as Serializable>::Size::USIZE * i
                    ..<T as Serializable>::Size::USIZE * (i + 1)],
            ));
        }
        self.credit.serialize(GenericArray::from_mut_slice(
            &mut buf[breakdown_key_size..breakdown_key_size + <T as Serializable>::Size::USIZE],
        ));
    }

    /// reads the bytes from `buf` into a `MCAggregateCreditOutputRow`
    /// # Panics
    /// if the `buf` is not exactly the right size
    #[must_use]
    pub fn deserialize(buf: &[u8]) -> Self {
        assert_eq!(buf.len(), Self::SIZE);
        let mut breakdown_key = Vec::with_capacity(BK::BITS as usize);
        for i in 0..BK::BITS as usize {
            breakdown_key.push(<T as Serializable>::deserialize(GenericArray::from_slice(
                &buf[<T as Serializable>::Size::USIZE * i
                    ..<T as Serializable>::Size::USIZE * (i + 1)],
            )));
        }
        let credit = <T as Serializable>::deserialize(GenericArray::from_slice(
            &buf[<T as Serializable>::Size::USIZE * BK::BITS as usize..],
        ));
        Self::new(breakdown_key, credit)
    }

    pub fn from_byte_slice(from: &[u8]) -> impl Iterator<Item = Self> + '_ {
        debug_assert!(from.len() % Self::SIZE == 0);

        from.chunks((BK::BITS as usize + 1) * <T as Serializable>::Size::USIZE)
            .map(|chunk| Self::deserialize(chunk))
    }
}

#[async_trait]
impl<F: Field, T: Arithmetic<F>> Resharable<F> for MCAccumulateCreditInputRow<F, T> {
    type Share = T;

    async fn reshare<C>(&self, ctx: C, record_id: RecordId, to_helper: Role) -> Result<Self, Error>
    where
        C: Context + LegacyReshare<F, Share = T> + Send,
    {
        let f_trigger_bit = ctx
            .narrow(&AttributionResharableStep::IsTriggerReport)
            .reshare(&self.is_trigger_report, record_id, to_helper);
        let f_helper_bit = ctx.narrow(&AttributionResharableStep::HelperBit).reshare(
            &self.helper_bit,
            record_id,
            to_helper,
        );
        let f_breakdown_key = self.breakdown_key.reshare(
            ctx.narrow(&AttributionResharableStep::BreakdownKey),
            record_id,
            to_helper,
        );
        let f_value = ctx
            .narrow(&AttributionResharableStep::TriggerValue)
            .reshare(&self.trigger_value, record_id, to_helper);

        let (breakdown_key, (is_trigger_report, helper_bit, trigger_value)) = try_join(
            f_breakdown_key,
            try_join3(f_trigger_bit, f_helper_bit, f_value),
        )
        .await?;

        Ok(MCAccumulateCreditInputRow::new(
            is_trigger_report,
            helper_bit,
            breakdown_key,
            trigger_value,
        ))
    }
}

#[async_trait]
impl<F: Field + Sized, T: Arithmetic<F>> Resharable<F> for MCCappedCreditsWithAggregationBit<F, T> {
    type Share = T;

    async fn reshare<C>(&self, ctx: C, record_id: RecordId, to_helper: Role) -> Result<Self, Error>
    where
        C: Context + LegacyReshare<F, Share = T> + Send,
    {
        let f_helper_bit = ctx.narrow(&AttributionResharableStep::HelperBit).reshare(
            &self.helper_bit,
            record_id,
            to_helper,
        );
        let f_aggregation_bit = ctx
            .narrow(&AttributionResharableStep::AggregationBit)
            .reshare(&self.aggregation_bit, record_id, to_helper);
        let f_breakdown_key = self.breakdown_key.reshare(
            ctx.narrow(&AttributionResharableStep::BreakdownKey),
            record_id,
            to_helper,
        );
        let f_value = ctx
            .narrow(&AttributionResharableStep::TriggerValue)
            .reshare(&self.credit, record_id, to_helper);

        let (breakdown_key, (helper_bit, aggregation_bit, credit)) = try_join(
            f_breakdown_key,
            try_join3(f_helper_bit, f_aggregation_bit, f_value),
        )
        .await?;

        Ok(MCCappedCreditsWithAggregationBit::new(
            helper_bit,
            aggregation_bit,
            breakdown_key,
            credit,
        ))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AttributionResharableStep {
    IsTriggerReport,
    HelperBit,
    BreakdownKey,
    TriggerValue,
    AggregationBit,
}

impl Substep for AttributionResharableStep {}

impl AsRef<str> for AttributionResharableStep {
    fn as_ref(&self) -> &str {
        match self {
            Self::IsTriggerReport => "is_trigger_report",
            Self::HelperBit => "helper_bit",
            Self::BreakdownKey => "breakdown_key",
            Self::TriggerValue => "trigger_value",
            Self::AggregationBit => "aggregation_bit",
        }
    }
}
