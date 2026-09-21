const x = [
  [],
  [0],
  [0, 0.5],
  [0, 0, 0.5],
  [0, 0.5, 0, 0.5]
]

const y = [
  [],
  [0],
  [0, 0],
  [0, 0.5, 0.5],
  [0, 0, 0.5, 0.5]
]

const width = [
  [],
  [1],
  [0.5, 0.5],
  [1, 0.5, 0.5],
  [0.5, 0.5, 0.5, 0.5]
]

const height = [
  [],
  [1],
  [1, 1],
  [0.5, 0.5, 0.5],
  [0.5, 0.5, 0.5, 0.5]
]

function getGamescopeClients() {
  var allClients = workspace.windowList();
  var gamescopeClients = [];

  for (var i = 0; i < allClients.length; i++) {
    if (
      allClients[i].resourceClass == "gamescope" ||
      allClients[i].resourceClass == "gamescope-kbm"
    ) {
      gamescopeClients.push(allClients[i]);
    }
  }
  return gamescopeClients;
}

function numGamescopeClientsInOutput(output) {
  var gamescopeClients = getGamescopeClients();
  var count = 0;
  for (var i = 0; i < gamescopeClients.length; i++) {
    if (gamescopeClients[i].output == output) {
      count++;
    }
  }
  return count;
}

function gamescopeAboveBelow() {
  var gamescopeClients = getGamescopeClients();
  for (var i = 0; i < gamescopeClients.length; i++) {
    if (
      workspace.activeWindow.resourceClass == "gamescope" ||
      workspace.activeWindow.resourceClass == "gamescope-kbm"
    ) {
      gamescopeClients[i].keepAbove = true;
    } else {
      gamescopeClients[i].keepAbove = false;
    }
  }
}

function gamescopeSplitscreen() {
  var gamescopeClients = getGamescopeClients();

  var screenMap = new Map();
  var screens = workspace.screens;
  for (var j = 0; j < screens.length; j++) {
    screenMap.set(screens[j], 0);
  }

  for (var i = 0; i < gamescopeClients.length; i++) {
    var monitor = gamescopeClients[i].output;
    var monitorX = monitor.geometry.x;
    var monitorY = monitor.geometry.y;
    var monitorWidth = monitor.geometry.width;
    var monitorHeight = monitor.geometry.height;

    var playerCount = numGamescopeClientsInOutput(monitor);
    var playerIndex = screenMap.get(monitor);
    screenMap.set(monitor, playerIndex + 1);

    gamescopeClients[i].noBorder = true;
    gamescopeClients[i].frameGeometry = {
      x: monitorX + x[playerCount][playerIndex] * monitorWidth,
      y: monitorY + y[playerCount][playerIndex] * monitorHeight,
      width: monitorWidth * width[playerCount][playerIndex],
      height: monitorHeight * height[playerCount][playerIndex],
    };
  }
  gamescopeAboveBelow();
}

function gamescopeSplitscreenDelayed() {
  var timer = new QTimer();
  timer.interval = 100;
  timer.singleShot = true;
  timer.timeout.connect(gamescopeSplitscreen);
  timer.start();
}

function gamescopeAboveBelownDelayed() {
  var timer = new QTimer();
  timer.interval = 100;
  timer.singleShot = true;
  timer.timeout.connect(gamescopeSplitscreen);
  timer.start();
}

workspace.windowAdded.connect(gamescopeSplitscreenDelayed);
workspace.windowRemoved.connect(gamescopeSplitscreenDelayed);
workspace.windowActivated.connect(gamescopeAboveBelownDelayed);
