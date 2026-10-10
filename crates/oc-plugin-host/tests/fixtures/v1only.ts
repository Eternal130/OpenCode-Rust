// A v1-only plugin: default-exports the server() hooks object shape —
// tolerated (loaded inert), never crashes the host.
export default {
  server() {
    return {};
  },
};
