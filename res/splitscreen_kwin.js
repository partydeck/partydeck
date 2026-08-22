const partydeckConfig = { targets: [], verticalTwoPlayer: false } // PARTYDECK_CONFIG
const hookedWindows = new Set()

const x = [
  [],
  [0],
  partydeckConfig.verticalTwoPlayer ? [0, 0.5] : [0, 0],
  [0, 0, 0.5],
  [0, 0.5, 0, 0.5]
]

const y = [
  [],
  [0],
  partydeckConfig.verticalTwoPlayer ? [0, 0] : [0, 0.5],
  [0, 0.5, 0.5],
  [0, 0, 0.5, 0.5]
]

const width = [
  [],
  [1],
  partydeckConfig.verticalTwoPlayer ? [0.5, 0.5] : [1, 1],
  [1, 0.5, 0.5],
  [0.5, 0.5, 0.5, 0.5]
]

const height = [
  [],
  [1],
  partydeckConfig.verticalTwoPlayer ? [1, 1] : [0.5, 0.5],
  [0.5, 0.5, 0.5],
  [0.5, 0.5, 0.5, 0.5]
]

function targetFor(window) {
  return partydeckConfig.targets.find(target => target.pid == window.pid) || null
}

function outputByName(name) {
  // SDL may append a model name while KWin exposes only the DRM connector.
  return workspace.screens.find(output =>
    name == output.name || name.startsWith(output.name + " ")
  ) || null
}

function hookWindow(window) {
  if (hookedWindows.has(window)) {
    return
  }
  hookedWindows.add(window)
  window.outputChanged.connect(gamescopeSplitscreen)
  window.frameGeometryChanged.connect(gamescopeSplitscreen)
  window.closed.connect(() => hookedWindows.delete(window))
}

function getGamescopeClients() {
  return workspace.stackingOrder.filter(window => targetFor(window) != null)
}

function gamescopeAboveBelow() {
  const activeWindow = workspace.activeWindow
  const keepAbove = activeWindow != null && targetFor(activeWindow) != null
  getGamescopeClients().forEach(window => window.keepAbove = keepAbove)
}

function gamescopeSplitscreen() {
  const outputClients = new Map()

  getGamescopeClients().forEach(client => {
    hookWindow(client)
    const target = targetFor(client)
    const output = outputByName(target.output)
    if (output == null) {
      print(
        "[partydeck] KWin placement failed: pid=" + client.pid +
        ", missing output=" + target.output
      )
      return
    }

    workspace.sendClientToScreen(client, output)
    const clients = outputClients.get(output) || []
    clients.push(client)
    outputClients.set(output, clients)
  })

  outputClients.forEach((clients, output) => {
    const playerCount = clients.length
    if (playerCount >= x.length) {
      print(
        "[partydeck] KWin placement failed: " + playerCount +
        " instances target output " + output.name + "; maximum is " + (x.length - 1)
      )
      return
    }

    const monitor = output.geometry
    clients.forEach((client, playerIndex) => {
      const fullScreen = playerCount == 1
      if (client.fullScreen != fullScreen) {
        client.fullScreen = fullScreen
      }
      client.noBorder = true
      const geometry = {
        x: monitor.x + x[playerCount][playerIndex] * monitor.width,
        y: monitor.y + y[playerCount][playerIndex] * monitor.height,
        width: monitor.width * width[playerCount][playerIndex],
        height: monitor.height * height[playerCount][playerIndex],
      }
      if (
        client.frameGeometry.x != geometry.x ||
        client.frameGeometry.y != geometry.y ||
        client.frameGeometry.width != geometry.width ||
        client.frameGeometry.height != geometry.height
      ) {
        client.frameGeometry = geometry
      }
    })
  })
  gamescopeAboveBelow()
}

workspace.windowAdded.connect(gamescopeSplitscreen)
workspace.windowRemoved.connect(gamescopeSplitscreen)
workspace.windowActivated.connect(gamescopeAboveBelow)
workspace.screensChanged.connect(gamescopeSplitscreen)
gamescopeSplitscreen()
