use crate::data::{BundleSet, ComponentSet};
use crate::dynamics::solver::joint_constraint::joint_generic_velocity_constraint::{
    JointGenericVelocityConstraint, JointGenericVelocityGroundConstraint,
};
use crate::dynamics::solver::joint_constraint::joint_velocity_constraint::{
    JointVelocityConstraint, JointVelocityGroundConstraint, SolverBody,
};
use crate::dynamics::solver::DeltaVel;
use crate::dynamics::{
    ImpulseJoint, IntegrationParameters, JointGraphEdge, JointIndex, RigidBodyIds,
    RigidBodyMassProps, RigidBodyPosition, RigidBodyType, RigidBodyVelocity,
};
#[cfg(feature = "simd-is-enabled")]
use crate::math::{Isometry, SimdReal, SIMD_WIDTH};
use crate::math::{Real, SPATIAL_DIM};
use crate::prelude::MultibodyJointSet;
use na::DVector;

pub enum AnyJointVelocityConstraint {
    JointConstraint(JointVelocityConstraint<Real, 1>),
    JointGroundConstraint(JointVelocityGroundConstraint<Real, 1>),
    JointGenericConstraint(JointGenericVelocityConstraint),
    JointGenericGroundConstraint(JointGenericVelocityGroundConstraint),
    #[cfg(feature = "simd-is-enabled")]
    JointConstraintSimd(JointVelocityConstraint<SimdReal, SIMD_WIDTH>),
    #[cfg(feature = "simd-is-enabled")]
    JointGroundConstraintSimd(JointVelocityGroundConstraint<SimdReal, SIMD_WIDTH>),
    Empty,
}

impl AnyJointVelocityConstraint {
    #[cfg(feature = "parallel")]
    pub fn num_active_constraints(_: &ImpulseJoint) -> usize {
        1
    }

    pub fn from_joint<Bodies>(
        params: &IntegrationParameters,
        joint_id: JointIndex,
        joint: &ImpulseJoint,
        bodies: &Bodies,
        multibodies: &MultibodyJointSet,
        j_id: &mut usize,
        jacobians: &mut DVector<Real>,
        out: &mut Vec<Self>,
    ) where
        Bodies: ComponentSet<RigidBodyPosition>
            + ComponentSet<RigidBodyVelocity>
            + ComponentSet<RigidBodyMassProps>
            + ComponentSet<RigidBodyIds>,
    {
        let local_frame1 = joint.data.local_frame1;
        let local_frame2 = joint.data.local_frame2;
        let rb1: (
            &RigidBodyPosition,
            &RigidBodyVelocity,
            &RigidBodyMassProps,
            &RigidBodyIds,
        ) = bodies.index_bundle(joint.body1.0);
        let rb2: (
            &RigidBodyPosition,
            &RigidBodyVelocity,
            &RigidBodyMassProps,
            &RigidBodyIds,
        ) = bodies.index_bundle(joint.body2.0);

        let (rb_pos1, rb_vel1, rb_mprops1, rb_ids1) = rb1;
        let (rb_pos2, rb_vel2, rb_mprops2, rb_ids2) = rb2;
        let frame1 = rb_pos1.position * local_frame1;
        let frame2 = rb_pos2.position * local_frame2;

        let body1 = SolverBody {
            linvel: rb_vel1.linvel,
            angvel: rb_vel1.angvel,
            im: rb_mprops1.effective_inv_mass,
            sqrt_ii: rb_mprops1.effective_world_inv_inertia_sqrt,
            world_com: rb_mprops1.world_com,
            mj_lambda: [rb_ids1.active_set_offset],
        };
        let body2 = SolverBody {
            linvel: rb_vel2.linvel,
            angvel: rb_vel2.angvel,
            im: rb_mprops2.effective_inv_mass,
            sqrt_ii: rb_mprops2.effective_world_inv_inertia_sqrt,
            world_com: rb_mprops2.world_com,
            mj_lambda: [rb_ids2.active_set_offset],
        };

        let mb1 = multibodies
            .rigid_body_link(joint.body1)
            .map(|link| (&multibodies[link.multibody], link.id));
        let mb2 = multibodies
            .rigid_body_link(joint.body2)
            .map(|link| (&multibodies[link.multibody], link.id));

        if mb1.is_some() || mb2.is_some() {
            let multibodies_ndof = mb1.map(|m| m.0.ndofs()).unwrap_or(SPATIAL_DIM)
                + mb2.map(|m| m.0.ndofs()).unwrap_or(SPATIAL_DIM);

            if multibodies_ndof == 0 {
                // Both multibodies are fixed, don’t generate any constraint.
                return;
            }

            // For each solver contact we generate up to SPATIAL_DIM constraints, and each
            // constraints appends the multibodies jacobian and weighted jacobians.
            // Also note that for impulse_joints, the rigid-bodies will also add their jacobians
            // to the generic DVector.
            // TODO: is this count correct when we take both motors and limits into account?
            let required_jacobian_len = *j_id + multibodies_ndof * 2 * SPATIAL_DIM;

            if jacobians.nrows() < required_jacobian_len {
                jacobians.resize_vertically_mut(required_jacobian_len, 0.0);
            }

            // TODO: find a way to avoid the temporary buffer.
            let mut out_tmp = [JointGenericVelocityConstraint::invalid(); 12];
            let out_tmp_len = JointGenericVelocityConstraint::lock_axes(
                params,
                joint_id,
                &body1,
                &body2,
                mb1,
                mb2,
                &frame1,
                &frame2,
                &joint.data,
                jacobians,
                j_id,
                &mut out_tmp,
            );

            for c in out_tmp.into_iter().take(out_tmp_len) {
                out.push(AnyJointVelocityConstraint::JointGenericConstraint(c));
            }
        } else {
            // TODO: find a way to avoid the temporary buffer.
            let mut out_tmp = [JointVelocityConstraint::invalid(); 12];
            let out_tmp_len = JointVelocityConstraint::<Real, 1>::lock_axes(
                params,
                joint_id,
                &body1,
                &body2,
                &frame1,
                &frame2,
                &joint.data,
                &mut out_tmp,
            );

            for c in out_tmp.into_iter().take(out_tmp_len) {
                out.push(AnyJointVelocityConstraint::JointConstraint(c));
            }
        }
    }

    #[cfg(feature = "simd-is-enabled")]
    pub fn from_wide_joint<Bodies>(
        params: &IntegrationParameters,
        joint_id: [JointIndex; SIMD_WIDTH],
        impulse_joints: [&ImpulseJoint; SIMD_WIDTH],
        bodies: &Bodies,
        out: &mut Vec<Self>,
    ) where
        Bodies: ComponentSet<RigidBodyPosition>
            + ComponentSet<RigidBodyVelocity>
            + ComponentSet<RigidBodyMassProps>
            + ComponentSet<RigidBodyIds>,
    {
        let rbs1: (
            [&RigidBodyPosition; SIMD_WIDTH],
            [&RigidBodyVelocity; SIMD_WIDTH],
            [&RigidBodyMassProps; SIMD_WIDTH],
            [&RigidBodyIds; SIMD_WIDTH],
        ) = (
            gather![|ii| bodies.index(impulse_joints[ii].body1.0)],
            gather![|ii| bodies.index(impulse_joints[ii].body1.0)],
            gather![|ii| bodies.index(impulse_joints[ii].body1.0)],
            gather![|ii| bodies.index(impulse_joints[ii].body1.0)],
        );
        let rbs2: (
            [&RigidBodyPosition; SIMD_WIDTH],
            [&RigidBodyVelocity; SIMD_WIDTH],
            [&RigidBodyMassProps; SIMD_WIDTH],
            [&RigidBodyIds; SIMD_WIDTH],
        ) = (
            gather![|ii| bodies.index(impulse_joints[ii].body2.0)],
            gather![|ii| bodies.index(impulse_joints[ii].body2.0)],
            gather![|ii| bodies.index(impulse_joints[ii].body2.0)],
            gather![|ii| bodies.index(impulse_joints[ii].body2.0)],
        );

        let (rb_pos1, rb_vel1, rb_mprops1, rb_ids1) = rbs1;
        let (rb_pos2, rb_vel2, rb_mprops2, rb_ids2) = rbs2;
        let pos1: Isometry<SimdReal> = gather![|ii| rb_pos1[ii].position].into();
        let pos2: Isometry<SimdReal> = gather![|ii| rb_pos2[ii].position].into();

        let local_frame1: Isometry<SimdReal> =
            gather![|ii| impulse_joints[ii].data.local_frame1].into();
        let local_frame2: Isometry<SimdReal> =
            gather![|ii| impulse_joints[ii].data.local_frame2].into();

        let frame1 = pos1 * local_frame1;
        let frame2 = pos2 * local_frame2;

        let body1: SolverBody<SimdReal, SIMD_WIDTH> = SolverBody {
            linvel: gather![|ii| rb_vel1[ii].linvel].into(),
            angvel: gather![|ii| rb_vel1[ii].angvel].into(),
            im: gather![|ii| rb_mprops1[ii].effective_inv_mass].into(),
            sqrt_ii: gather![|ii| rb_mprops1[ii].effective_world_inv_inertia_sqrt].into(),
            world_com: gather![|ii| rb_mprops1[ii].world_com].into(),
            mj_lambda: gather![|ii| rb_ids1[ii].active_set_offset],
        };
        let body2: SolverBody<SimdReal, SIMD_WIDTH> = SolverBody {
            linvel: gather![|ii| rb_vel2[ii].linvel].into(),
            angvel: gather![|ii| rb_vel2[ii].angvel].into(),
            im: gather![|ii| rb_mprops2[ii].effective_inv_mass].into(),
            sqrt_ii: gather![|ii| rb_mprops2[ii].effective_world_inv_inertia_sqrt].into(),
            world_com: gather![|ii| rb_mprops2[ii].world_com].into(),
            mj_lambda: gather![|ii| rb_ids2[ii].active_set_offset],
        };

        // TODO: find a way to avoid the temporary buffer.
        let mut out_tmp = [JointVelocityConstraint::invalid(); 12];
        let out_tmp_len = JointVelocityConstraint::<SimdReal, SIMD_WIDTH>::lock_axes(
            params,
            joint_id,
            &body1,
            &body2,
            &frame1,
            &frame2,
            impulse_joints[0].data.locked_axes.bits(),
            &mut out_tmp,
        );

        for c in out_tmp.into_iter().take(out_tmp_len) {
            out.push(AnyJointVelocityConstraint::JointConstraintSimd(c));
        }
    }

    pub fn from_joint_ground<Bodies>(
        params: &IntegrationParameters,
        joint_id: JointIndex,
        joint: &ImpulseJoint,
        bodies: &Bodies,
        multibodies: &MultibodyJointSet,
        j_id: &mut usize,
        jacobians: &mut DVector<Real>,
        out: &mut Vec<Self>,
    ) where
        Bodies: ComponentSet<RigidBodyPosition>
            + ComponentSet<RigidBodyType>
            + ComponentSet<RigidBodyVelocity>
            + ComponentSet<RigidBodyMassProps>
            + ComponentSet<RigidBodyIds>,
    {
        let mut handle1 = joint.body1;
        let mut handle2 = joint.body2;
        let status2: &RigidBodyType = bodies.index(handle2.0);
        let flipped = !status2.is_dynamic();

        let (local_frame1, local_frame2) = if flipped {
            std::mem::swap(&mut handle1, &mut handle2);
            (joint.data.local_frame2, joint.data.local_frame1)
        } else {
            (joint.data.local_frame1, joint.data.local_frame2)
        };

        let rb1: (&RigidBodyPosition, &RigidBodyVelocity, &RigidBodyMassProps) =
            bodies.index_bundle(handle1.0);
        let rb2: (
            &RigidBodyPosition,
            &RigidBodyVelocity,
            &RigidBodyMassProps,
            &RigidBodyIds,
        ) = bodies.index_bundle(handle2.0);

        let (rb_pos1, rb_vel1, rb_mprops1) = rb1;
        let (rb_pos2, rb_vel2, rb_mprops2, rb_ids2) = rb2;
        let frame1 = rb_pos1.position * local_frame1;
        let frame2 = rb_pos2.position * local_frame2;

        let body1 = SolverBody {
            linvel: rb_vel1.linvel,
            angvel: rb_vel1.angvel,
            im: rb_mprops1.effective_inv_mass,
            sqrt_ii: rb_mprops1.effective_world_inv_inertia_sqrt,
            world_com: rb_mprops1.world_com,
            mj_lambda: [crate::INVALID_USIZE],
        };
        let body2 = SolverBody {
            linvel: rb_vel2.linvel,
            angvel: rb_vel2.angvel,
            im: rb_mprops2.effective_inv_mass,
            sqrt_ii: rb_mprops2.effective_world_inv_inertia_sqrt,
            world_com: rb_mprops2.world_com,
            mj_lambda: [rb_ids2.active_set_offset],
        };

        if let Some(mb2) = multibodies
            .rigid_body_link(handle2)
            .map(|link| (&multibodies[link.multibody], link.id))
        {
            let multibodies_ndof = mb2.0.ndofs();

            if multibodies_ndof == 0 {
                // The multibody is fixed, don’t generate any constraint.
                return;
            }

            // For each solver contact we generate up to SPATIAL_DIM constraints, and each
            // constraints appends the multibodies jacobian and weighted jacobians.
            // Also note that for impulse_joints, the rigid-bodies will also add their jacobians
            // to the generic DVector.
            // TODO: is this count correct when we take both motors and limits into account?
            let required_jacobian_len = *j_id + multibodies_ndof * 2 * SPATIAL_DIM;

            if jacobians.nrows() < required_jacobian_len {
                jacobians.resize_vertically_mut(required_jacobian_len, 0.0);
            }

            // TODO: find a way to avoid the temporary buffer.
            let mut out_tmp = [JointGenericVelocityGroundConstraint::invalid(); 12];
            let out_tmp_len = JointGenericVelocityGroundConstraint::lock_axes(
                params,
                joint_id,
                &body1,
                &body2,
                mb2,
                &frame1,
                &frame2,
                &joint.data,
                jacobians,
                j_id,
                &mut out_tmp,
            );

            for c in out_tmp.into_iter().take(out_tmp_len) {
                out.push(AnyJointVelocityConstraint::JointGenericGroundConstraint(c));
            }
        } else {
            // TODO: find a way to avoid the temporary buffer.
            let mut out_tmp = [JointVelocityGroundConstraint::invalid(); 12];
            let out_tmp_len = JointVelocityGroundConstraint::<Real, 1>::lock_axes(
                params,
                joint_id,
                &body1,
                &body2,
                &frame1,
                &frame2,
                &joint.data,
                &mut out_tmp,
            );

            for c in out_tmp.into_iter().take(out_tmp_len) {
                out.push(AnyJointVelocityConstraint::JointGroundConstraint(c));
            }
        }
    }

    #[cfg(feature = "simd-is-enabled")]
    pub fn from_wide_joint_ground<Bodies>(
        params: &IntegrationParameters,
        joint_id: [JointIndex; SIMD_WIDTH],
        impulse_joints: [&ImpulseJoint; SIMD_WIDTH],
        bodies: &Bodies,
        out: &mut Vec<Self>,
    ) where
        Bodies: ComponentSet<RigidBodyPosition>
            + ComponentSet<RigidBodyType>
            + ComponentSet<RigidBodyVelocity>
            + ComponentSet<RigidBodyMassProps>
            + ComponentSet<RigidBodyIds>,
    {
        let mut handles1 = gather![|ii| impulse_joints[ii].body1];
        let mut handles2 = gather![|ii| impulse_joints[ii].body2];
        let status2: [&RigidBodyType; SIMD_WIDTH] = gather![|ii| bodies.index(handles2[ii].0)];
        let mut flipped = [false; SIMD_WIDTH];

        for ii in 0..SIMD_WIDTH {
            if !status2[ii].is_dynamic() {
                std::mem::swap(&mut handles1[ii], &mut handles2[ii]);
                flipped[ii] = true;
            }
        }

        let local_frame1: Isometry<SimdReal> = gather![|ii| if flipped[ii] {
            impulse_joints[ii].data.local_frame2
        } else {
            impulse_joints[ii].data.local_frame1
        }]
        .into();
        let local_frame2: Isometry<SimdReal> = gather![|ii| if flipped[ii] {
            impulse_joints[ii].data.local_frame1
        } else {
            impulse_joints[ii].data.local_frame2
        }]
        .into();

        let rbs1: (
            [&RigidBodyPosition; SIMD_WIDTH],
            [&RigidBodyVelocity; SIMD_WIDTH],
            [&RigidBodyMassProps; SIMD_WIDTH],
        ) = (
            gather![|ii| bodies.index(handles1[ii].0)],
            gather![|ii| bodies.index(handles1[ii].0)],
            gather![|ii| bodies.index(handles1[ii].0)],
        );
        let rbs2: (
            [&RigidBodyPosition; SIMD_WIDTH],
            [&RigidBodyVelocity; SIMD_WIDTH],
            [&RigidBodyMassProps; SIMD_WIDTH],
            [&RigidBodyIds; SIMD_WIDTH],
        ) = (
            gather![|ii| bodies.index(handles2[ii].0)],
            gather![|ii| bodies.index(handles2[ii].0)],
            gather![|ii| bodies.index(handles2[ii].0)],
            gather![|ii| bodies.index(handles2[ii].0)],
        );

        let (rb_pos1, rb_vel1, rb_mprops1) = rbs1;
        let (rb_pos2, rb_vel2, rb_mprops2, rb_ids2) = rbs2;
        let pos1: Isometry<SimdReal> = gather![|ii| rb_pos1[ii].position].into();
        let pos2: Isometry<SimdReal> = gather![|ii| rb_pos2[ii].position].into();

        let frame1 = pos1 * local_frame1;
        let frame2 = pos2 * local_frame2;

        let body1: SolverBody<SimdReal, SIMD_WIDTH> = SolverBody {
            linvel: gather![|ii| rb_vel1[ii].linvel].into(),
            angvel: gather![|ii| rb_vel1[ii].angvel].into(),
            im: gather![|ii| rb_mprops1[ii].effective_inv_mass].into(),
            sqrt_ii: gather![|ii| rb_mprops1[ii].effective_world_inv_inertia_sqrt].into(),
            world_com: gather![|ii| rb_mprops1[ii].world_com].into(),
            mj_lambda: [crate::INVALID_USIZE; SIMD_WIDTH],
        };
        let body2: SolverBody<SimdReal, SIMD_WIDTH> = SolverBody {
            linvel: gather![|ii| rb_vel2[ii].linvel].into(),
            angvel: gather![|ii| rb_vel2[ii].angvel].into(),
            im: gather![|ii| rb_mprops2[ii].effective_inv_mass].into(),
            sqrt_ii: gather![|ii| rb_mprops2[ii].effective_world_inv_inertia_sqrt].into(),
            world_com: gather![|ii| rb_mprops2[ii].world_com].into(),
            mj_lambda: gather![|ii| rb_ids2[ii].active_set_offset],
        };

        // TODO: find a way to avoid the temporary buffer.
        let mut out_tmp = [JointVelocityGroundConstraint::invalid(); 12];
        let out_tmp_len = JointVelocityGroundConstraint::<SimdReal, SIMD_WIDTH>::lock_axes(
            params,
            joint_id,
            &body1,
            &body2,
            &frame1,
            &frame2,
            impulse_joints[0].data.locked_axes.bits(),
            &mut out_tmp,
        );

        for c in out_tmp.into_iter().take(out_tmp_len) {
            out.push(AnyJointVelocityConstraint::JointGroundConstraintSimd(c));
        }
    }

    pub fn remove_bias_from_rhs(&mut self) {
        match self {
            AnyJointVelocityConstraint::JointConstraint(c) => c.remove_bias_from_rhs(),
            AnyJointVelocityConstraint::JointGroundConstraint(c) => c.remove_bias_from_rhs(),
            #[cfg(feature = "simd-is-enabled")]
            AnyJointVelocityConstraint::JointConstraintSimd(c) => c.remove_bias_from_rhs(),
            #[cfg(feature = "simd-is-enabled")]
            AnyJointVelocityConstraint::JointGroundConstraintSimd(c) => c.remove_bias_from_rhs(),
            AnyJointVelocityConstraint::JointGenericConstraint(c) => c.remove_bias_from_rhs(),
            AnyJointVelocityConstraint::JointGenericGroundConstraint(c) => c.remove_bias_from_rhs(),
            AnyJointVelocityConstraint::Empty => unreachable!(),
        }
    }

    pub fn solve(
        &mut self,
        jacobians: &DVector<Real>,
        mj_lambdas: &mut [DeltaVel<Real>],
        generic_mj_lambdas: &mut DVector<Real>,
    ) {
        match self {
            AnyJointVelocityConstraint::JointConstraint(c) => c.solve(mj_lambdas),
            AnyJointVelocityConstraint::JointGroundConstraint(c) => c.solve(mj_lambdas),
            #[cfg(feature = "simd-is-enabled")]
            AnyJointVelocityConstraint::JointConstraintSimd(c) => c.solve(mj_lambdas),
            #[cfg(feature = "simd-is-enabled")]
            AnyJointVelocityConstraint::JointGroundConstraintSimd(c) => c.solve(mj_lambdas),
            AnyJointVelocityConstraint::JointGenericConstraint(c) => {
                c.solve(jacobians, mj_lambdas, generic_mj_lambdas)
            }
            AnyJointVelocityConstraint::JointGenericGroundConstraint(c) => {
                c.solve(jacobians, mj_lambdas, generic_mj_lambdas)
            }
            AnyJointVelocityConstraint::Empty => unreachable!(),
        }
    }

    pub fn writeback_impulses(&self, joints_all: &mut [JointGraphEdge]) {
        match self {
            AnyJointVelocityConstraint::JointConstraint(c) => c.writeback_impulses(joints_all),
            AnyJointVelocityConstraint::JointGroundConstraint(c) => {
                c.writeback_impulses(joints_all)
            }
            #[cfg(feature = "simd-is-enabled")]
            AnyJointVelocityConstraint::JointConstraintSimd(c) => c.writeback_impulses(joints_all),
            #[cfg(feature = "simd-is-enabled")]
            AnyJointVelocityConstraint::JointGroundConstraintSimd(c) => {
                c.writeback_impulses(joints_all)
            }
            AnyJointVelocityConstraint::JointGenericConstraint(c) => {
                c.writeback_impulses(joints_all)
            }
            AnyJointVelocityConstraint::JointGenericGroundConstraint(c) => {
                c.writeback_impulses(joints_all)
            }
            AnyJointVelocityConstraint::Empty => unreachable!(),
        }
    }
}
