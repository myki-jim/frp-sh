# Website motion system

The homepage now uses a procedural Canvas optical field in `ConnectionField.vue`, replacing the generated hardware image. No raster hero is loaded. The earlier generated asset was removed at the user's request.

GSAP provides headline masks, text illumination, scroll progress, perspective rings, connection-path drawing, section masks, rotating scene orbits and desktop horizontal storytelling. Canvas caps pixel density at 1.5 and pauses offscreen or when the tab is hidden. Reduced-motion preference renders a static field and vertical story layout.

The EN / 中文 button switches the complete landing page in place and remembers the explicit choice in local storage. First visits default to English. Documentation links follow the selected language. HTML language metadata follows the rendered language; install commands remain unchanged.

Validated with a production VitePress build and browser checks: both language directions, persistence after reload, translated document links, desktop view, and a 390 px Chinese mobile layout without horizontal overflow. Browser console reported no errors or warnings during the check.