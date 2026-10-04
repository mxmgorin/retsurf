// LCD grid: dark lines between the game's pixels.

#if defined(VERTEX)

#if __VERSION__ >= 130
#define COMPAT_VARYING out
#define COMPAT_ATTRIBUTE in
#else
#define COMPAT_VARYING varying
#define COMPAT_ATTRIBUTE attribute
#endif

COMPAT_ATTRIBUTE vec4 VertexCoord;
COMPAT_ATTRIBUTE vec4 TexCoord;
COMPAT_VARYING vec2 vTexCoord;
uniform mat4 MVPMatrix;

void main() {
    gl_Position = MVPMatrix * VertexCoord;
    vTexCoord = TexCoord.xy;
}

#elif defined(FRAGMENT)

#ifdef GL_ES
precision mediump float;
#endif

#if __VERSION__ >= 130
#define COMPAT_VARYING in
#define COMPAT_TEXTURE texture
out vec4 FragColor;
#else
#define COMPAT_VARYING varying
#define COMPAT_TEXTURE texture2D
#define FragColor gl_FragColor
#endif

uniform sampler2D Texture;
uniform vec2 TextureSize;
uniform vec2 OutputSize;
COMPAT_VARYING vec2 vTexCoord;

// Line darkness, and the gain that restores the average brightness.
const float DEPTH = 0.4;
const float GAIN = 1.25;

void main() {
    vec2 texel = vTexCoord * TextureSize;
    vec4 color = COMPAT_TEXTURE(Texture, (floor(texel) + 0.5) / TextureSize);
    // The last output row and column of each game pixel are the grid.
    vec2 scale = OutputSize / TextureSize;
    vec2 cell = floor(fract(texel) * scale);
    vec2 line = step(scale - 1.0, cell);
    FragColor = vec4(min(color.rgb * (1.0 - DEPTH * max(line.x, line.y)) * GAIN, vec3(1.0)), 1.0);
}
#endif
