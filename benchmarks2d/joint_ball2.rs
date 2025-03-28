use rapier2d::prelude::*;
use rapier_testbed2d::Testbed;

pub fn init_world(testbed: &mut Testbed) {
    /*
     * World
     */
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut impulse_joints = ImpulseJointSet::new();
    let multibody_joints = MultibodyJointSet::new();

    /*
     * Create the balls
     */
    // Build the rigid body.
    let rad = 0.4;
    let numi = 100; // Num vertical nodes.
    let numk = 100; // Num horizontal nodes.
    let shift = 1.0;

    let mut body_handles = Vec::new();

    for k in 0..numk {
        for i in 0..numi {
            let fk = k as f32;
            let fi = i as f32;

            let status = if k >= numk / 2 - 3 && k <= numk / 2 + 3 && i == 0 {
                RigidBodyType::Static
            } else {
                RigidBodyType::Dynamic
            };

            let rigid_body = RigidBodyBuilder::new(status)
                .translation(vector![fk * shift, -fi * shift])
                .build();
            let child_handle = bodies.insert(rigid_body);
            let collider = ColliderBuilder::ball(rad).build();
            colliders.insert_with_parent(collider, child_handle, &mut bodies);

            // Vertical joint.
            if i > 0 {
                let parent_handle = *body_handles.last().unwrap();
                let joint = RevoluteJoint::new().local_anchor2(point![0.0, shift]);
                impulse_joints.insert(parent_handle, child_handle, joint);
            }

            // Horizontal joint.
            if k > 0 {
                let parent_index = body_handles.len() - numi;
                let parent_handle = body_handles[parent_index];
                let joint = RevoluteJoint::new().local_anchor2(point![-shift, 0.0]);
                impulse_joints.insert(parent_handle, child_handle, joint);
            }

            body_handles.push(child_handle);
        }
    }

    /*
     * Set up the testbed.
     */
    testbed.set_world(bodies, colliders, impulse_joints, multibody_joints);
    testbed.look_at(point![numk as f32 * rad, numi as f32 * -rad], 5.0);
}
