// Prepended to the internal passes, so they run on GLES 2, GLES 3 and GL.
#ifdef GL_ES
    precision mediump float;
#endif

#if NEW_SHADER_INTERFACE
    #define I in
    out vec4 f_color;
    #define texture2D texture
#else
    #define I varying
    #define f_color gl_FragColor
#endif
