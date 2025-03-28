use crate::{
    physics::{PhysicsEvents, PhysicsState},
    TestbedGraphics,
};
use plugin::HarnessPlugin;
use rapier::dynamics::{
    CCDSolver, ImpulseJointSet, IntegrationParameters, IslandManager, MultibodyJointSet,
    RigidBodySet,
};
use rapier::geometry::{BroadPhase, ColliderSet, NarrowPhase};
use rapier::math::{Real, Vector};
use rapier::pipeline::{ChannelEventCollector, PhysicsHooks, PhysicsPipeline, QueryPipeline};

pub mod plugin;

pub struct RunState {
    #[cfg(feature = "parallel")]
    pub thread_pool: rapier::rayon::ThreadPool,
    pub num_threads: usize,
    pub timestep_id: usize,
    pub time: f32,
}

impl RunState {
    pub fn new() -> Self {
        #[cfg(feature = "parallel")]
        let num_threads = num_cpus::get_physical();
        #[cfg(not(feature = "parallel"))]
        let num_threads = 1;

        #[cfg(feature = "parallel")]
        let thread_pool = rapier::rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap();

        Self {
            #[cfg(feature = "parallel")]
            thread_pool: thread_pool,
            num_threads,
            timestep_id: 0,
            time: 0.0,
        }
    }
}

pub struct Harness {
    pub physics: PhysicsState,
    max_steps: usize,
    callbacks: Callbacks,
    plugins: Vec<Box<dyn HarnessPlugin>>,
    events: PhysicsEvents,
    event_handler: ChannelEventCollector,
    pub state: RunState,
}

type Callbacks =
    Vec<Box<dyn FnMut(Option<&mut TestbedGraphics>, &mut PhysicsState, &PhysicsEvents, &RunState)>>;

#[allow(dead_code)]
impl Harness {
    pub fn new_empty() -> Self {
        let contact_channel = crossbeam::channel::unbounded();
        let proximity_channel = crossbeam::channel::unbounded();
        let event_handler = ChannelEventCollector::new(proximity_channel.0, contact_channel.0);
        let events = PhysicsEvents {
            contact_events: contact_channel.1,
            intersection_events: proximity_channel.1,
        };
        let physics = PhysicsState::new();
        let state = RunState::new();

        Self {
            physics,
            max_steps: 1000,
            callbacks: Vec::new(),
            plugins: Vec::new(),
            events,
            event_handler,
            state,
        }
    }

    pub fn new(
        bodies: RigidBodySet,
        colliders: ColliderSet,
        impulse_joints: ImpulseJointSet,
        multibody_joints: MultibodyJointSet,
    ) -> Self {
        let mut res = Self::new_empty();
        res.set_world(bodies, colliders, impulse_joints, multibody_joints);
        res
    }

    pub fn set_max_steps(&mut self, max_steps: usize) {
        self.max_steps = max_steps
    }

    pub fn integration_parameters_mut(&mut self) -> &mut IntegrationParameters {
        &mut self.physics.integration_parameters
    }

    pub fn clear_callbacks(&mut self) {
        self.callbacks.clear();
    }

    pub fn physics_state_mut(&mut self) -> &mut PhysicsState {
        &mut self.physics
    }

    pub fn set_world(
        &mut self,
        bodies: RigidBodySet,
        colliders: ColliderSet,
        impulse_joints: ImpulseJointSet,
        multibody_joints: MultibodyJointSet,
    ) {
        self.set_world_with_params(
            bodies,
            colliders,
            impulse_joints,
            multibody_joints,
            Vector::y() * -9.81,
            (),
        )
    }

    pub fn set_world_with_params(
        &mut self,
        bodies: RigidBodySet,
        colliders: ColliderSet,
        impulse_joints: ImpulseJointSet,
        multibody_joints: MultibodyJointSet,
        gravity: Vector<Real>,
        hooks: impl PhysicsHooks<RigidBodySet, ColliderSet> + 'static,
    ) {
        // println!("Num bodies: {}", bodies.len());
        // println!("Num impulse_joints: {}", impulse_joints.len());
        self.physics.gravity = gravity;
        self.physics.bodies = bodies;
        self.physics.colliders = colliders;
        self.physics.impulse_joints = impulse_joints;
        self.physics.multibody_joints = multibody_joints;
        self.physics.hooks = Box::new(hooks);

        self.physics.islands = IslandManager::new();
        self.physics.broad_phase = BroadPhase::new();
        self.physics.narrow_phase = NarrowPhase::new();
        self.state.timestep_id = 0;
        self.state.time = 0.0;
        self.physics.ccd_solver = CCDSolver::new();
        self.physics.query_pipeline = QueryPipeline::new();
        self.physics.pipeline = PhysicsPipeline::new();
        self.physics.pipeline.counters.enable();
    }

    pub fn add_plugin(&mut self, plugin: impl HarnessPlugin + 'static) {
        self.plugins.push(Box::new(plugin));
    }

    pub fn add_callback<
        F: FnMut(Option<&mut TestbedGraphics>, &mut PhysicsState, &PhysicsEvents, &RunState) + 'static,
    >(
        &mut self,
        callback: F,
    ) {
        self.callbacks.push(Box::new(callback));
    }

    pub fn step(&mut self) {
        self.step_with_graphics(None);
    }

    pub fn step_with_graphics(&mut self, mut graphics: Option<&mut TestbedGraphics>) {
        #[cfg(feature = "parallel")]
        {
            let physics = &mut self.physics;
            let event_handler = &self.event_handler;
            self.state.thread_pool.install(|| {
                physics.pipeline.step(
                    &physics.gravity,
                    &physics.integration_parameters,
                    &mut physics.islands,
                    &mut physics.broad_phase,
                    &mut physics.narrow_phase,
                    &mut physics.bodies,
                    &mut physics.colliders,
                    &mut physics.impulse_joints,
                    &mut physics.multibody_joints,
                    &mut physics.ccd_solver,
                    &*physics.hooks,
                    event_handler,
                );
            });
        }

        #[cfg(not(feature = "parallel"))]
        self.physics.pipeline.step(
            &self.physics.gravity,
            &self.physics.integration_parameters,
            &mut self.physics.islands,
            &mut self.physics.broad_phase,
            &mut self.physics.narrow_phase,
            &mut self.physics.bodies,
            &mut self.physics.colliders,
            &mut self.physics.impulse_joints,
            &mut self.physics.multibody_joints,
            &mut self.physics.ccd_solver,
            &*self.physics.hooks,
            &self.event_handler,
        );

        self.physics.query_pipeline.update(
            &self.physics.islands,
            &self.physics.bodies,
            &self.physics.colliders,
        );

        for plugin in &mut self.plugins {
            plugin.step(&mut self.physics, &self.state)
        }

        for f in &mut self.callbacks {
            f(
                graphics.as_deref_mut(),
                &mut self.physics,
                &self.events,
                &self.state,
            );
        }

        for plugin in &mut self.plugins {
            plugin.run_callbacks(&mut self.physics, &self.events, &self.state)
        }

        self.events.poll_all();

        self.state.time += self.physics.integration_parameters.dt as f32;
        self.state.timestep_id += 1;
    }

    pub fn run(&mut self) {
        for _ in 0..self.max_steps {
            self.step();
        }
    }
}
