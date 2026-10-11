// The PTY sidebar fixture: a v2 plugin whose SETUP claims the
// `sidebar.content` slot seam at boot — `ctx.ui.sidebar.setSection`
// lands its section in the host's UI-request queue without any event
// (the per-pump drain applies it before the first session frame).
export default {
  id: "sidebarboot",
  async setup(ctx) {
    await ctx.ui.sidebar.setSection({
      title: "Agents",
      rows: ["build · running", "librarian · idle"],
    });
  },
};
