export const chapters = [
  ["Your desk. A room for everyone.", "你的桌面，大家的房间。"],
  ["One code. An open invitation.", "一个房间号，邀请大家加入。"],
  ["Different places. One room.", "身在各处，共处一室。"],
  ["Together, on the same network.", "一起，连进同一个网络。"],
  ["Build together. Play together.", "一起创造，一起开玩。"],
  [
    "Direct when possible. Relay when needed.",
    "能够直连，就直达。需要时，中继接力。",
  ],
  ["Less in the way.", "让工具退到幕后。"],
  ["Your next room starts here.", "你的下一个房间，从这里开始。"],
];
export const stops = [0, 0.13, 0.3, 0.47, 0.61, 0.75, 0.85, 1];
export const commands = {
  unix: "curl -fsSL https://frp.sh/install.sh | sh",
  windows: "irm https://frp.sh/install.ps1 | iex",
};
export function chapterAt(p) {
  for (let i = stops.length - 1; i >= 0; i--)
    if (p >= stops[i] - 0.035) return i;
  return 0;
}
