// The unit quad stretched over the viewport.
#if NEW_SHADER_INTERFACE
    #define I in
    #define O out
#else
    #define I attribute
    #define O varying
#endif

I vec2 a_pos;
O vec2 v_tc;

void main() {
    v_tc = a_pos;
    gl_Position = vec4(a_pos * 2.0 - 1.0, 0.0, 1.0);
}
