use crate::config::token_enum::token_enum;
use crate::platform::render::shaders::{self, Rows};

token_enum! {
    /// The rows a page shader sees the game in: fewer than the game draws leave
    /// room between them for the shader's scanlines and masks.
    pub enum ShaderResolution {
        default Auto;
        Auto => "auto", "Auto",
        Native => "native", "Native",
        R480 => "480", "480",
        R360 => "360", "360",
        R240 => "240", "240",
    }
}

impl ShaderResolution {
    /// The rows for `shader`; Auto takes the scale the shader is made for.
    pub fn rows(self, shader: &str) -> Rows {
        match self {
            ShaderResolution::Auto => shaders::preferred_rows(shader),
            ShaderResolution::Native => Rows::Native,
            ShaderResolution::R480 => Rows::Fixed(480),
            ShaderResolution::R360 => Rows::Fixed(360),
            ShaderResolution::R240 => Rows::Fixed(240),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_the_shader() {
        assert_eq!(ShaderResolution::Auto.rows("zfast-crt"), Rows::Scale(4));
        assert_eq!(ShaderResolution::Auto.rows("sharp-bilinear"), Rows::Native);
        assert_eq!(ShaderResolution::Native.rows("zfast-crt"), Rows::Native);
        assert_eq!(
            ShaderResolution::R360.rows("sharp-bilinear"),
            Rows::Fixed(360)
        );
    }
}
