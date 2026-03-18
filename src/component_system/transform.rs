use cgmath::Rotation3;

pub struct Transform {
    pub position: cgmath::Vector3<f32>,
    pub rotation: cgmath::Quaternion<f32>,
    pub scale: cgmath::Vector3<f32>,
}

impl Transform {
    pub fn new(position: [f32; 3]) -> Self {
            Self {
                position: position.into(),
                rotation:  cgmath::Quaternion::from_angle_z(cgmath::Deg(0.0)), // No rotation initially
                scale:  cgmath::Vector3::new(1.0, 1.0, 1.0),
            }
        }

    pub fn to_matrix(&self) -> cgmath::Matrix4<f32> {
        let pos = cgmath::Matrix4::from_translation(self.position);
        let rot = cgmath::Matrix4::from(self.rotation);
        let scale = cgmath::Matrix4::from_nonuniform_scale(self.scale.x, self.scale.y, self.scale.z);
        
        pos * rot * scale // Scale first, then Rotate, then Translate
    }
}