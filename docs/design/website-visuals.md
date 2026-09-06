# Website: a room for everyone

The landing page is one continuous Three.js world. `RoomLanding.vue` manages scrolling, language, light preferences and installation actions; `room-story.js` defines the eight story stops; `room-world.js` owns models, cameras, printed textures, lighting and physically registered interactive surfaces.

Six rooms have distinct structures and props: a wood study, pitched loft, industrial studio, gaming bedroom, greenhouse and electronics workshop. A shared-code demonstration grows from one to six illustrated members, then brings their floors together. Six is the composition's member count, not a capacity limit. Moving signals, animated local-service/game screens and a relay detour explain the connection. These are illustrations rather than measured live performance.

Day, night and system modes affect sunlight, ambient light and individual lamps; transitions pass through a warm intermediate state. The choice persists independently of language. First visits default to English; Chinese text replaces the same physical labels and screen surfaces. No generated bitmap hero or old two-room component remains.

The final camera faces the original monitor. Its selectable text is attached to the monitor in 3D using CSS3DRenderer. Modeled keycaps carry invisible semantic hit surfaces for platform switching, clipboard copy and documentation/source navigation. Ray tests suppress occluded interactive surfaces. Introductory and descriptive text is printed on physical objects or displayed on modeled screens. Keyboard and screen-reader alternatives remain available; the normal visual page has no conventional feature cards or floating headings.

Geometry and materials are reused, pixel density is capped at 1.5, shadow rendering only updates when needed, and hidden tabs pause rendering. Reduced motion snaps to the eight camera stops and disables continuous animation. WebGL failure retains installation instructions. Mobile uses a taller monitor and larger text at the final stop.

Validation performed: production VitePress build, desktop and 390px mobile browser inspection, forward scrolling and direct installation jump, OS switching and clipboard feedback, English/Chinese switching, and persisted language/light mode after reload. No browser errors were observed during the tested interactions. Large-bundle warnings remain; no measured frame-rate or download-speed claim is made.

Design reference and narrative: see `3d-room-storyboard.md`. The original inspiration was the interactive workspace concept described by https://office.graffico.it/, linked from the GSAP showcase. No source artwork or models were copied.
