use super::*;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct LookupAndMemoryArgumentLayout {
    pub intermediate_polys_for_range_check_16: OptimizedOraclesForLookupWidth1,
    pub remainder_for_range_check_16: Option<AlignedColumnSet<4>>,
    pub lazy_init_address_range_check_16: Option<OptimizedOraclesForLookupWidth1>,
    pub intermediate_polys_for_timestamp_range_checks: OptimizedOraclesForLookupWidth1,
    pub intermediate_polys_for_generic_lookup: AlignedColumnSet<4>,
    pub intermediate_poly_for_range_check_16_multiplicity: AlignedColumnSet<4>,
    pub intermediate_poly_for_timestamp_range_check_multiplicity: AlignedColumnSet<4>,
    pub intermediate_polys_for_generic_multiplicities: AlignedColumnSet<4>,
    pub delegation_processing_aux_poly: Option<AlignedColumnSet<4>>,
    pub intermediate_polys_for_memory_argument: AlignedColumnSet<4>,
    pub ext4_polys_offset: usize,
    pub total_width: usize,
}

impl LookupAndMemoryArgumentLayout {
    pub fn get_intermediate_polys_for_generic_lookup_absolute_poly_idx_for_verifier(
        &self,
        idx: usize,
    ) -> usize {
        let poly_idx = self
            .intermediate_polys_for_generic_lookup
            .get_range(idx)
            .start
            - self.ext4_polys_offset;
        assert_eq!(poly_idx % 4, 0);
        let poly_idx = poly_idx / 4;

        self.num_base_field_polys() + poly_idx
    }

    pub fn range_check_16_intermediate_poly_for_multiplicities_absolute_poly_idx_for_verifier(
        &self,
    ) -> usize {
        let poly_idx = self
            .intermediate_poly_for_range_check_16_multiplicity
            .get_range(0)
            .start
            - self.ext4_polys_offset;
        assert_eq!(poly_idx % 4, 0);
        let poly_idx = poly_idx / 4;

        self.num_base_field_polys() + poly_idx
    }

    pub fn timestamp_range_check_intermediate_poly_for_multiplicities_absolute_poly_idx_for_verifier(
        &self,
    ) -> usize {
        let poly_idx = self
            .intermediate_poly_for_timestamp_range_check_multiplicity
            .get_range(0)
            .start
            - self.ext4_polys_offset;
        assert_eq!(poly_idx % 4, 0);
        let poly_idx = poly_idx / 4;

        self.num_base_field_polys() + poly_idx
    }

    pub fn generic_width_3_lookup_intermediate_polys_for_multiplicities_absolute_poly_idx_for_verifier(
        &self,
        idx: usize,
    ) -> usize {
        let poly_idx = self
            .intermediate_polys_for_generic_multiplicities
            .get_range(idx)
            .start
            - self.ext4_polys_offset;
        assert_eq!(poly_idx % 4, 0);
        let poly_idx = poly_idx / 4;

        self.num_base_field_polys() + poly_idx
    }

    pub const fn get_intermediate_polys_for_memory_argument_absolute_poly_idx_for_verifier(
        &self,
        idx: usize,
    ) -> usize {
        let poly_idx = self
            .intermediate_polys_for_memory_argument
            .get_range(idx)
            .start
            - self.ext4_polys_offset;
        assert!(poly_idx % 4 == 0);
        let poly_idx = poly_idx / 4;

        self.num_base_field_polys() + poly_idx
    }

    pub fn get_aux_polys_for_gelegation_argument_absolute_poly_idx_for_verifier(
        &self,
    ) -> Option<usize> {
        let Some(delegation_processing_aux_poly) = self.delegation_processing_aux_poly else {
            return None;
        };

        let poly_idx = delegation_processing_aux_poly.start - self.ext4_polys_offset;
        assert_eq!(poly_idx % 4, 0);
        let poly_idx = poly_idx / 4;

        let poly_num = self.num_base_field_polys() + poly_idx;

        Some(poly_num)
    }

    pub fn from_compiled_parts<F: PrimeField>(
        witness_layout: &WitnessSubtree<F>,
        memory_layout: &MemorySubtree,
        setup_layout: &SetupLayout,
    ) -> Self {
        let total_number_of_range_check_16_exprs =
            witness_layout.range_check_16_lookup_expressions.len();

        let num_timestamp_range_checks = witness_layout
            .timestamp_range_check_lookup_expressions
            .len();
        assert_eq!(num_timestamp_range_checks % 2, 0);

        // we want to layout all our aux base field polys together as they will need to be adjusted to c0==0 for commitment
        let num_base_field_aux_polys_range_check_16 = total_number_of_range_check_16_exprs / 2;
        let num_base_field_aux_polys_timestamp_range_checks = num_timestamp_range_checks / 2;
        let needs_extra_ext4_poly_for_range_check_16 =
            total_number_of_range_check_16_exprs % 2 != 0;

        let mut offset = 0;
        let base_field_oracles_range_check_16 =
            AlignedColumnSet::layout_at(&mut offset, num_base_field_aux_polys_range_check_16);

        let base_field_oracles_range_check_16_for_lazy_init_address =
            if let Some(lazy_init_and_teardown) =
                memory_layout.shuffle_ram_inits_and_teardowns.as_ref()
            {
                assert_eq!(
                    lazy_init_and_teardown
                        .lazy_init_addresses_columns
                        .num_elements(),
                    1,
                );
                assert_eq!(
                    lazy_init_and_teardown.lazy_init_addresses_columns.width(),
                    2
                );

                let base_field_oracles_range_check_16_for_lazy_init_address =
                    AlignedColumnSet::layout_at(&mut offset, 1);

                Some(base_field_oracles_range_check_16_for_lazy_init_address)
            } else {
                None
            };

        let base_field_oracles_for_timestamp_range_checks = AlignedColumnSet::layout_at(
            &mut offset,
            num_base_field_aux_polys_timestamp_range_checks,
        );

        let ext_4_field_oracles_range_check_16 =
            AlignedColumnSet::layout_at(&mut offset, num_base_field_aux_polys_range_check_16);
        let ext4_polys_offset = ext_4_field_oracles_range_check_16.start();

        let ext_4_field_oracles_range_check_16_for_lazy_init_address =
            if memory_layout.shuffle_ram_inits_and_teardowns.is_some() {
                let ext_4_field_oracles_range_check_16_for_lazy_init_address =
                    AlignedColumnSet::layout_at(&mut offset, 1);

                Some(ext_4_field_oracles_range_check_16_for_lazy_init_address)
            } else {
                None
            };

        let lazy_init_address_range_check_16 =
            if let Some(base_field_oracles_range_check_16_for_lazy_init_address) =
                base_field_oracles_range_check_16_for_lazy_init_address
            {
                let Some(ext_4_field_oracles_range_check_16_for_lazy_init_address) =
                    ext_4_field_oracles_range_check_16_for_lazy_init_address
                else {
                    unreachable!()
                };

                let lazy_init_address_range_check_16 = OptimizedOraclesForLookupWidth1 {
                    num_pairs: 1,
                    base_field_oracles: base_field_oracles_range_check_16_for_lazy_init_address,
                    ext_4_field_oracles: ext_4_field_oracles_range_check_16_for_lazy_init_address,
                };

                Some(lazy_init_address_range_check_16)
            } else {
                None
            };

        let remainder_for_range_check_16 = if needs_extra_ext4_poly_for_range_check_16 {
            let remainder_for_range_check_16 = AlignedColumnSet::layout_at(&mut offset, 1);
            Some(remainder_for_range_check_16)
        } else {
            None
        };

        let ext_4_field_oracles_for_timestamp_range_checks = AlignedColumnSet::layout_at(
            &mut offset,
            num_base_field_aux_polys_timestamp_range_checks,
        );

        let intermediate_polys_for_generic_lookup =
            AlignedColumnSet::layout_at(&mut offset, witness_layout.width_3_lookups.len());

        let intermediate_poly_for_range_check_16_multiplicity =
            AlignedColumnSet::layout_at(&mut offset, 1);

        let intermediate_poly_for_timestamp_range_check_multiplicity =
            AlignedColumnSet::layout_at(&mut offset, 1);

        let intermediate_polys_for_generic_multiplicities = AlignedColumnSet::layout_at(
            &mut offset,
            setup_layout.generic_lookup_setup_columns.num_elements(),
        );

        let delegation_processing_aux_poly = if let Some(_delegation_processing_columns) =
            memory_layout.delegation_request_layout
        {
            let delegation_processing_aux_poly = AlignedColumnSet::layout_at(&mut offset, 1);

            Some(delegation_processing_aux_poly)
        } else if let Some(_delegation_processor_layout) = memory_layout.delegation_processor_layout
        {
            let delegation_processing_aux_poly = AlignedColumnSet::layout_at(&mut offset, 1);

            Some(delegation_processing_aux_poly)
        } else {
            None
        };

        // since we use constraint degree 2, we will always perform accumulations/definitions
        // as P(x) = a/b for lazy init/teardown,
        // or as Q(x) = P(x) * a/b for memory accesses in cycle
        // or R(x*omega) = R(x) * Q(x) for final accumulation

        let intermediate_polys_for_memory_argument =
            if memory_layout.shuffle_ram_access_sets.is_empty() == false {
                assert!(
                    memory_layout.batched_ram_accesses.is_empty()
                        && memory_layout.register_and_indirect_accesses.is_empty()
                );
                // init/teardown accumulators + intermediate accumulators per access + grand product accumulator
                let num_set_polys_for_memory_shuffle =
                    1 + memory_layout.shuffle_ram_access_sets.len() + 1;

                AlignedColumnSet::layout_at(&mut offset, num_set_polys_for_memory_shuffle)
            } else {
                // no lazy init here
                assert!(
                    memory_layout.batched_ram_accesses.is_empty() == false
                        || memory_layout.register_and_indirect_accesses.is_empty() == false
                );

                // no lazy init here to count
                let num_intermediate_polys_for_batched_ram =
                    memory_layout.batched_ram_accesses.len();
                let mut num_intermediate_polys_for_register_or_indirect_accesses =
                    memory_layout.register_and_indirect_accesses.len();
                for el in memory_layout.register_and_indirect_accesses.iter() {
                    num_intermediate_polys_for_register_or_indirect_accesses +=
                        el.indirect_accesses.len();
                }
                // intermediate accumulators per access + grand product accumulator
                let num_set_polys_for_memory_shuffle = num_intermediate_polys_for_batched_ram
                    + num_intermediate_polys_for_register_or_indirect_accesses
                    + 1;

                AlignedColumnSet::layout_at(&mut offset, num_set_polys_for_memory_shuffle)
            };

        let intermediate_polys_for_range_check_16 = OptimizedOraclesForLookupWidth1 {
            num_pairs: num_base_field_aux_polys_range_check_16,
            base_field_oracles: base_field_oracles_range_check_16,
            ext_4_field_oracles: ext_4_field_oracles_range_check_16,
        };

        let intermediate_polys_for_timestamp_range_checks = OptimizedOraclesForLookupWidth1 {
            num_pairs: num_base_field_aux_polys_timestamp_range_checks,
            base_field_oracles: base_field_oracles_for_timestamp_range_checks,
            ext_4_field_oracles: ext_4_field_oracles_for_timestamp_range_checks,
        };

        Self {
            intermediate_polys_for_range_check_16,
            remainder_for_range_check_16,
            lazy_init_address_range_check_16,
            intermediate_polys_for_timestamp_range_checks,
            intermediate_polys_for_generic_lookup,
            intermediate_polys_for_memory_argument,
            delegation_processing_aux_poly,
            intermediate_poly_for_range_check_16_multiplicity,
            intermediate_poly_for_timestamp_range_check_multiplicity,
            intermediate_polys_for_generic_multiplicities,
            total_width: offset,
            ext4_polys_offset,
        }
    }

    pub const fn num_base_field_polys(&self) -> usize {
        let els = if let Some(el) = self.lazy_init_address_range_check_16 {
            el.num_pairs
        } else {
            0
        };
        self.intermediate_polys_for_range_check_16.num_pairs
            + self.intermediate_polys_for_timestamp_range_checks.num_pairs
            + els
    }

    pub const fn num_ext4_field_polys(&self) -> usize {
        let lazy_init_els = if let Some(el) = self.lazy_init_address_range_check_16 {
            el.num_pairs
        } else {
            0
        };

        let delegation_set_sum_els = if let Some(_) = self.delegation_processing_aux_poly {
            1
        } else {
            0
        };

        self.intermediate_polys_for_range_check_16.num_pairs
            + lazy_init_els
            + self.intermediate_polys_for_timestamp_range_checks.num_pairs
            + delegation_set_sum_els
            + self.intermediate_polys_for_generic_lookup.num_elements()
            + self
                .intermediate_poly_for_range_check_16_multiplicity
                .num_elements()
            + self
                .intermediate_poly_for_timestamp_range_check_multiplicity
                .num_elements()
            + self
                .intermediate_polys_for_generic_multiplicities
                .num_elements()
            + self.intermediate_polys_for_memory_argument.num_elements()
    }
}
