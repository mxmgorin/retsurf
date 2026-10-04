// SameBoy's LCD shader: an RGB subpixel grid, ported to RetroArch's single-pass GLSL from
// https://github.com/LIJI32/SameBoy/blob/c458e7c5d2d350fb37a1931c40da9f758d28d240/Shaders/LCD.fsh
//
// Copyright (c) 2015-2026 Lior Halphon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

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
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
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

// SameBoy's MasterShader.fsh, cut to what scale() needs: it works in linear
// light, and its images run top-down where this one runs bottom-up.
#define STATIC
#define GAMMA 2.2

vec4 sb_texture(sampler2D t, vec2 pos)
{
    return pow(COMPAT_TEXTURE(t, vec2(pos.x, 1.0 - pos.y)), vec4(GAMMA));
}

vec4 texture_relative(sampler2D t, vec2 pos, vec2 offset)
{
    return sb_texture(t, (floor(pos * TextureSize) + offset + vec2(0.5, 0.5)) / TextureSize);
}

#define texture sb_texture

// SameBoy's Shaders/LCD.fsh, unmodified:
#define COLOR_LOW 0.6
#define COLOR_HIGH 1.0
#define SCANLINE_DEPTH 0.2

STATIC vec4 scale(sampler2D image, vec2 position, vec2 input_resolution, vec2 output_resolution)
{
    vec2 pos = fract(position * input_resolution);
    vec2 sub_pos = pos * 6.0;
    
    vec4 center = texture_relative(image, position, vec2(0, 0));
    vec4 left = texture_relative(image, position, vec2(-1, 0));
    vec4 right = texture_relative(image, position, vec2(1, 0));
    
    if (sub_pos.y < 1.0) {
        center = mix(center, texture_relative(image, position, vec2( 0, -1)), 0.5 - sub_pos.y / 2.0);
        left =   mix(left,   texture_relative(image, position, vec2(-1, -1)), 0.5 - sub_pos.y / 2.0);
        right =  mix(right,  texture_relative(image, position, vec2( 1, -1)), 0.5 - sub_pos.y / 2.0);
        center *= sub_pos.y * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
        left *= sub_pos.y * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
        right *= sub_pos.y * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
    }
    else if (sub_pos.y > 5.0) {
        center = mix(center, texture_relative(image, position, vec2( 0, 1)), (sub_pos.y - 5.0) / 2.0);
        left =   mix(left,   texture_relative(image, position, vec2(-1, 1)), (sub_pos.y - 5.0) / 2.0);
        right =  mix(right,  texture_relative(image, position, vec2( 1, 1)), (sub_pos.y - 5.0) / 2.0);
        center *= (6.0 - sub_pos.y) * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
        left *= (6.0 - sub_pos.y) * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
        right *= (6.0 - sub_pos.y) * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
    }
    
    
    vec4 midleft = mix(left, center, 0.5);
    vec4 midright = mix(right, center, 0.5);
    
    vec4 ret;
    if (sub_pos.x < 1.0) {
        ret = mix(vec4(COLOR_HIGH * center.r, COLOR_LOW * center.g, COLOR_HIGH * left.b, 1),
                  vec4(COLOR_HIGH * center.r, COLOR_LOW * center.g, COLOR_LOW  * left.b, 1),
                  sub_pos.x);
    }
    else if (sub_pos.x < 2.0) {
        ret = mix(vec4(COLOR_HIGH * center.r, COLOR_LOW  * center.g, COLOR_LOW * left.b, 1),
                  vec4(COLOR_HIGH * center.r, COLOR_HIGH * center.g, COLOR_LOW * midleft.b, 1),
                  sub_pos.x - 1.0);
    }
    else if (sub_pos.x < 3.0) {
        ret = mix(vec4(COLOR_HIGH * center.r  , COLOR_HIGH * center.g, COLOR_LOW * midleft.b, 1),
                  vec4(COLOR_LOW  * midright.r, COLOR_HIGH * center.g, COLOR_LOW * center.b, 1),
                  sub_pos.x - 2.0);
    }
    else if (sub_pos.x < 4.0) {
        ret = mix(vec4(COLOR_LOW * midright.r, COLOR_HIGH * center.g , COLOR_LOW  * center.b, 1),
                  vec4(COLOR_LOW * right.r   , COLOR_HIGH  * center.g, COLOR_HIGH * center.b, 1),
                  sub_pos.x - 3.0);
    }
    else if (sub_pos.x < 5.0) {
        ret = mix(vec4(COLOR_LOW * right.r, COLOR_HIGH * center.g  , COLOR_HIGH * center.b, 1),
                  vec4(COLOR_LOW * right.r, COLOR_LOW  * midright.g, COLOR_HIGH * center.b, 1),
                  sub_pos.x - 4.0);
    }
    else {
        ret = mix(vec4(COLOR_LOW  * right.r, COLOR_LOW * midright.g, COLOR_HIGH * center.b, 1),
                  vec4(COLOR_HIGH * right.r, COLOR_LOW * right.g  ,  COLOR_HIGH * center.b, 1),
                  sub_pos.x - 5.0);
    }
    
    return ret;
}

void main()
{
    vec2 position = vec2(vTexCoord.x, 1.0 - vTexCoord.y);
    FragColor = pow(scale(Texture, position, TextureSize, OutputSize), vec4(1.0 / GAMMA));
}

#endif
