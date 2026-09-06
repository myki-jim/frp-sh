# Website direction: readable content, supporting 3D

This supersedes the fully immersive storyboard at the user's request. Normal page text and controls now handle the introduction, language and appearance selection, documentation links, OS selection and clipboard copying. There are no 3D controls, camera fly-throughs, moving rooms or large signs inside the models.

`RoomLanding.vue` renders the page. `studios.js` creates a fixed orthographic illustration of six separated workspaces. The geometry is newly constructed with smooth rounded edges, window openings, desk/chair clearance and distinct furnishing layouts. Room positions and the camera do not animate. Small signal markers are the only continuous scene motion; lighting transitions between day and night.

English remains the first-visit default; Chinese and light mode preferences persist. Reduced-motion preference suppresses automatic motion. Geometry and materials are reused, offscreen/hidden rendering pauses, and the 3D module loads separately. WebGL unavailability leaves all page content and installation controls usable.

Validation: production build; desktop render inspection; day/night and language controls; OS selection and copy feedback. The 3D view is decorative and does not report real network performance. Large bundle warnings remain.
