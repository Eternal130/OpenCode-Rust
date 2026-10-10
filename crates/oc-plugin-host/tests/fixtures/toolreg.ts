// Corpus fixture (tool-registry.test.ts "logs and skips invalid
// registrations"): one healthy tool beside an invalid name (rejected
// GUEST-side by the editor), an invalid dotted namespace and the
// reserved direct `execute` (rejected HOST-side by the registration
// validation) — only the healthy one registers.
export default {
  id: "toolreg",
  async setup(ctx) {
    let guestRejected = false;
    await ctx.tool.transform(async (editor) => {
      editor.add({
        name: "healthy",
        description: "The healthy one",
        input: { type: "object", properties: {} },
        async execute() {
          return "ok";
        },
      });
      try {
        editor.add({ name: "", description: "empty name", input: {} });
      } catch {
        guestRejected = true; // the editor's own guard (v2 editor.add throws)
      }
      editor.add({
        name: "nested",
        description: "bad namespace",
        input: {},
        options: { namespace: "slack..admin" },
      });
      editor.add({
        name: "execute",
        description: "reserved for CodeMode",
        input: {},
        options: { codemode: false },
      });
    });
    ctx.log(`toolreg: guest rejected empty name = ${guestRejected}`);
    return async () => {};
  },
};
