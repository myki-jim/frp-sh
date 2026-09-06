# Website motion system

The homepage uses a procedural Three.js scene in `RoomScene.vue`: two miniature workspaces with modeled desks, monitors, chairs, windows, lamps and plants. Scrolling draws a connection path, moves the camera and brings the rooms together while lowering the dividing wall. The scene illustrates the connection concept, not live network telemetry.

The direction takes inspiration from the interactive workspace concept described at https://office.graffico.it/, linked by the GSAP showcase. The supplied showcase page 4 was inaccessible during research. No artwork or model assets were copied. The old generated-image/particle-field implementation has been removed.

GSAP controls headline masks, section reveals, the pinned connection sequence and horizontal use cases. Narrow or short screens use a vertical layout and an explicit play button. Reduced-motion preference disables automatic motion and pinning. Three.js caps pixel density at 1.5, pauses when offscreen or hidden, disposes resources on navigation, and supplies a text fallback when WebGL is unavailable.

First visits default to English. EN / 中文 switches the complete page in place, remembers the choice, updates HTML language metadata and changes documentation links. Installation commands are available for both supported platform groups.

Validation: production VitePress build; desktop scene and connection replay; horizontal scrolling; 390 px mobile layout without horizontal overflow; language switching and persistence. No browser console errors were observed. The build still reports large JavaScript chunks, including Three.js and the documentation's Mermaid dependency; these are not a claim of measured load-time performance.
