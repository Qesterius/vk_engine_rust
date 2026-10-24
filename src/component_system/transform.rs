use std::ops::Mul;

use bevy_ecs::component::Component;
use cgmath::{Matrix4, Rotation3, SquareMatrix, Vector3};

/// Position, rotation and scale relative to the parent (or the world for roots); written by gameplay.
#[derive(Component)]
#[require(GlobalTransform)]
pub struct LocalTransform {
    pub position: cgmath::Vector3<f32>,
    pub rotation: cgmath::Quaternion<f32>,
    pub scale: cgmath::Vector3<f32>,
}

impl LocalTransform {
    pub fn new(position: [f32; 3]) -> Self {
        Self {
            position: position.into(),
            rotation: cgmath::Quaternion::from_angle_z(cgmath::Deg(0.0)), // No rotation initially
            scale: cgmath::Vector3::new(1.0, 1.0, 1.0),
        }
    }

    pub fn to_matrix(&self) -> cgmath::Matrix4<f32> {
        let pos = cgmath::Matrix4::from_translation(self.position);
        let rot = cgmath::Matrix4::from(self.rotation);
        let scale =
            cgmath::Matrix4::from_nonuniform_scale(self.scale.x, self.scale.y, self.scale.z);

        pos * rot * scale // Scale first, then Rotate, then Translate
    }

    /// Moves the transform by a relative offset
    pub fn translate(&mut self, offset: cgmath::Vector3<f32>) -> &mut Self {
        self.position += offset;
        self
    }

    #[allow(dead_code)]
    pub fn set_position(&mut self, position: [f32; 3]) -> &mut Self {
        self.position = position.into();
        self
    }

    #[allow(dead_code)]
    pub fn rotate_y(&mut self, degrees: f32) -> &mut Self {
        use cgmath::Rotation3;
        let rotation = cgmath::Quaternion::from_angle_y(cgmath::Deg(degrees));
        self.rotation = self.rotation * rotation;
        self
    }

    #[allow(dead_code)]
    pub fn set_scale(&mut self, s: f32) -> &mut Self {
        self.scale = cgmath::Vector3::new(s, s, s);
        self
    }

}

/// World-space matrix computed from the hierarchy; read-only outside this module.
#[derive(Component, Clone, Copy)]
pub struct GlobalTransform(Matrix4<f32>);

impl Default for GlobalTransform {
    fn default() -> Self {
        Self(Matrix4::identity())
    }

}

impl GlobalTransform {
    pub fn matrix(&self) -> Matrix4<f32> {
        self.0
    }

    #[allow(dead_code)]
    pub fn translation(&self) -> Vector3<f32> {
        self.0.w.truncate()
    }
}

impl Mul<&LocalTransform> for GlobalTransform {
    type Output = GlobalTransform;

    fn mul(self, rhs: &LocalTransform) -> Self::Output {
        let local_matrix = rhs.to_matrix();
        GlobalTransform(self.0 * local_matrix)
    }
}
