// Loaded by PartyDeck. Two tokens are replaced at launch:
//   PARTYDECK_ASSIGNMENTS      -> array mapping instance index -> screen index,
//                                 e.g. [0, 1].
//   PARTYDECK_USE_ASSIGNMENTS  -> true  : place each instance on its assigned
//                                         screen (multi-monitor);
//                                 false : classic splitscreen — ignore
//                                         assignments and split whatever screen
//                                         KWin opened each window on.
// Instances sharing a screen are split (vertical: side by side).
const assignments = PARTYDECK_ASSIGNMENTS;
const useAssignments = PARTYDECK_USE_ASSIGNMENTS;

const x = [[], [0], [0, 0.5], [0, 0, 0.5], [0, 0.5, 0, 0.5]];
const y = [[], [0], [0, 0], [0, 0.5, 0.5], [0, 0, 0.5, 0.5]];
const width = [[], [1], [0.5, 0.5], [1, 0.5, 0.5], [0.5, 0.5, 0.5, 0.5]];
const height = [[], [1], [1, 1], [0.5, 0.5, 0.5], [0.5, 0.5, 0.5, 0.5]];

function gamescopeClients() {
  const all = workspace.windowList();
  const out = [];
  for (let i = 0; i < all.length; i++) {
    const rc = all[i].resourceClass;
    if (rc == "gamescope" || rc == "gamescope-kbm") out.push(all[i]);
  }
  return out;
}

// The screen index a given client should live on. Multi-monitor mode honors the
// baked-in per-instance assignment; classic mode uses the screen KWin already
// placed the window on (so it just splits the current screen).
function screenIndexFor(clients, i, screens) {
  if (useAssignments) {
    return i < assignments.length ? assignments[i] : 0;
  }
  const out = clients[i].output;
  for (let s = 0; s < screens.length; s++) {
    if (screens[s] === out) return s;
  }
  return 0;
}

// Re-place every gamescope window on each event (robust to windows whose class
// isn't set the instant they're added). Window i (in list order) == instance i.
function layout() {
  const clients = gamescopeClients();
  const screens = workspace.screens;

  const total = {};
  for (let i = 0; i < clients.length; i++) {
    const s = screenIndexFor(clients, i, screens);
    total[s] = (total[s] || 0) + 1;
  }

  const slot = {};
  for (let i = 0; i < clients.length; i++) {
    const s = screenIndexFor(clients, i, screens);
    if (slot[s] === undefined) slot[s] = 0;
    const idx = slot[s];
    slot[s] += 1;
    const c = total[s];
    const screen = s >= 0 && s < screens.length ? screens[s] : screens[0];
    if (!screen) continue;
    const g = screen.geometry;
    const w = clients[i];
    w.noBorder = true;
    w.keepAbove = true;
    w.frameGeometry = {
      x: g.x + x[c][idx] * g.width,
      y: g.y + y[c][idx] * g.height,
      width: g.width * width[c][idx],
      height: g.height * height[c][idx],
    };
  }
}

workspace.windowAdded.connect(layout);
workspace.windowRemoved.connect(layout);
