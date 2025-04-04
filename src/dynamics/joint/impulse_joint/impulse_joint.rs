use crate::dynamics::{JointData, JointHandle, RigidBodyHandle};
use crate::math::{Real, SpacialVector};

#[cfg_attr(feature = "serde-serialize", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq)]
/// A joint attached to two bodies.
pub struct ImpulseJoint {
    /// Handle to the first body attached to this joint.
    pub body1: RigidBodyHandle,
    /// Handle to the second body attached to this joint.
    pub body2: RigidBodyHandle,

    pub data: JointData,
    pub impulses: SpacialVector<Real>,

    // A joint needs to know its handle to simplify its removal.
    pub(crate) handle: JointHandle,
    #[cfg(feature = "parallel")]
    pub(crate) constraint_index: usize,
}
