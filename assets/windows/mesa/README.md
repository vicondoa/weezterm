This is the prebuilt MSVC Mesa3D for Windows distribution from
https://github.com/pal1000/mesa-dist-win (release 26.0.6).

Two files are required and must be deployed TOGETHER, in the
same directory as `weezterm-gui.exe`:
- `opengl32.dll`       -- 137 KB; small WGL runtime / loader
- `libgallium_wgl.dll` -- the gallium megadriver (llvmpipe etc.)

Since Mesa 21.3.0 the megadriver was split out of `opengl32.dll`
to support multiple desktop GL drivers (llvmpipe, softpipe,
GLonD3D12). We force `GALLIUM_DRIVER=llvmpipe` at runtime when
routing OpenGL via Mesa to avoid Mesa picking GLonD3D12 (which on
RDP would go right back through WARP).

Mesa's License text can be found here:
https://docs.mesa3d.org/license.html
(a mixture of largely MITish licenses)

The old mesa.fdossena.com build only supported up to OpenGL 3.3
and lacked the libgallium_wgl.dll split that newer Mesa releases
require.
