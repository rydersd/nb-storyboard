export const viewportPayload = (view, viewedCut) => ({ pan_x: view.x, pan_y: view.y, zoom: view.z, viewed_cut: viewedCut || null });
