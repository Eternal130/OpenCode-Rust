// The click-probe TUI entry: registers a sidebar_content slot whose
// tree carries one CLICKABLE row (an onMouseUp prop — the same shape
// the flagship's decorateInteractiveRow produces through the host's
// @opentui shims) and one plain row. The activate handler navigates
// through api.route.navigate — the v2 TuiPluginApi surface.
const el = (tag, props, children = []) => ({ tag, props, children });
export default {
  id: "clickprobe:tui",
  async tui(api) {
    api.slots.register({
      order: 1,
      slots: {
        sidebar_content: () =>
          el("box", { flexDirection: "column" }, [
            el("box", { flexDirection: "row" }, [el("#text", { text: "Probe" })]),
            el(
              "box",
              {
                flexDirection: "row",
                onMouseUp: () => api.route.navigate("session", { sessionID: "ses_target" }),
              },
              [el("#text", { text: "Go to child" })],
            ),
            el("box", { flexDirection: "row" }, [el("#text", { text: "plain row" })]),
          ]),
      },
    });
  },
};
