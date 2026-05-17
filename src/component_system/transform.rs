use bevy_ecs::component::Component;
use cgmath::Rotation3;

#[derive(Component)]
pub struct Transform {
    pub position: cgmath::Vector3<f32>,
    pub rotation: cgmath::Quaternion<f32>,
    pub scale: cgmath::Vector3<f32>,
}

impl Transform {
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
