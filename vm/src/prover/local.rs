use std::{collections::VecDeque, marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use ax_stark_backend::{
    config::{Com, Domain, PcsProof, PcsProverData, StarkGenericConfig, Val},
    keygen::types::MultiStarkProvingKey,
    prover::types::Proof,
};
use ax_stark_sdk::{config::FriParameters, engine::StarkFriEngine};
use p3_field::PrimeField32;

use crate::{
    arch::{hasher::poseidon2::vm_poseidon2_hasher, VirtualMachine, VmConfig},
    prover::{
        AsyncContinuationVmProver, AsyncSingleSegmentVmProver, ContinuationVmProof,
        ContinuationVmProver, SingleSegmentVmProver,
    },
    system::{
        memory::tree::public_values::UserPublicValuesProof, program::trace::AxVmCommittedExe,
    },
};

pub struct VmLocalProver<SC: StarkGenericConfig, E: StarkFriEngine<SC>>
where
    Domain<SC>: Send + Sync,
    PcsProverData<SC>: Send + Sync,
    Com<SC>: Send + Sync,
    SC::Challenge: Send + Sync,
    PcsProof<SC>: Send + Sync,
{
    pub fri_parameters: FriParameters,
    pub vm_config: VmConfig,
    pub vm_pk: MultiStarkProvingKey<SC>,
    pub committed_exe: Arc<AxVmCommittedExe<SC>>,
    _marker: PhantomData<E>,
}

impl<SC: StarkGenericConfig, E: StarkFriEngine<SC>> VmLocalProver<SC, E>
where
    Domain<SC>: Send + Sync,
    PcsProverData<SC>: Send + Sync,
    Com<SC>: Send + Sync,
    SC::Challenge: Send + Sync,
    PcsProof<SC>: Send + Sync,
{
    pub fn new(
        fri_parameters: FriParameters,
        vm_config: VmConfig,
        vm_pk: MultiStarkProvingKey<SC>,
        committed_exe: Arc<AxVmCommittedExe<SC>>,
    ) -> Self {
        Self {
            fri_parameters,
            vm_config,
            vm_pk,
            committed_exe,
            _marker: PhantomData,
        }
    }
}

impl<SC: StarkGenericConfig, E: StarkFriEngine<SC>> ContinuationVmProver<SC>
    for VmLocalProver<SC, E>
where
    Domain<SC>: Send + Sync,
    PcsProverData<SC>: Send + Sync,
    Com<SC>: Send + Sync,
    SC::Challenge: Send + Sync,
    PcsProof<SC>: Send + Sync,
    Val<SC>: PrimeField32,
{
    fn prove(&self, input: impl Into<VecDeque<Vec<Val<SC>>>>) -> ContinuationVmProof<SC> {
        assert!(self.vm_config.continuation_enabled);
        let e = E::new(self.fri_parameters);
        let vm = VirtualMachine::new(e, self.vm_config.clone());
        let results = vm
            .execute_and_generate_with_cached_program(self.committed_exe.clone(), input)
            .unwrap();
        let user_public_values = UserPublicValuesProof::compute(
            self.vm_config.memory_config.memory_dimensions(),
            self.vm_config.num_public_values,
            &vm_poseidon2_hasher(),
            results.final_memory.as_ref().unwrap(),
        );
        let per_segment = vm.prove(&self.vm_pk, results);
        ContinuationVmProof {
            per_segment,
            user_public_values,
        }
    }
}

#[async_trait]
impl<SC: StarkGenericConfig, E: StarkFriEngine<SC>> AsyncContinuationVmProver<SC>
    for VmLocalProver<SC, E>
where
    VmLocalProver<SC, E>: Send + Sync,
    Domain<SC>: Send + Sync,
    PcsProverData<SC>: Send + Sync,
    Com<SC>: Send + Sync,
    SC::Challenge: Send + Sync,
    PcsProof<SC>: Send + Sync,
    Val<SC>: PrimeField32,
{
    async fn prove(
        &self,
        input: impl Into<VecDeque<Vec<Val<SC>>>> + Send + Sync,
    ) -> ContinuationVmProof<SC> {
        ContinuationVmProver::prove(self, input)
    }
}

impl<SC: StarkGenericConfig, E: StarkFriEngine<SC>> SingleSegmentVmProver<SC>
    for VmLocalProver<SC, E>
where
    Domain<SC>: Send + Sync,
    PcsProverData<SC>: Send + Sync,
    Com<SC>: Send + Sync,
    SC::Challenge: Send + Sync,
    PcsProof<SC>: Send + Sync,
    Val<SC>: PrimeField32,
{
    fn prove(&self, input: impl Into<VecDeque<Vec<Val<SC>>>>) -> Proof<SC> {
        assert!(!self.vm_config.continuation_enabled);
        let e = E::new(self.fri_parameters);
        let vm = VirtualMachine::new(e, self.vm_config.clone());
        let mut results = vm
            .execute_and_generate_with_cached_program(self.committed_exe.clone(), input)
            .unwrap();
        let segment = results.per_segment.pop().unwrap();
        vm.prove_single(&self.vm_pk, segment)
    }
}

#[async_trait]
impl<SC: StarkGenericConfig, E: StarkFriEngine<SC>> AsyncSingleSegmentVmProver<SC>
    for VmLocalProver<SC, E>
where
    VmLocalProver<SC, E>: Send + Sync,
    Domain<SC>: Send + Sync,
    PcsProverData<SC>: Send + Sync,
    Com<SC>: Send + Sync,
    SC::Challenge: Send + Sync,
    PcsProof<SC>: Send + Sync,
    Val<SC>: PrimeField32,
{
    async fn prove(&self, input: impl Into<VecDeque<Vec<Val<SC>>>> + Send + Sync) -> Proof<SC> {
        SingleSegmentVmProver::prove(self, input)
    }
}