// Wave-4 fuzz fixture: setup itself sleeps past the registration
// timeout — the load must fail contained (no crash attribution, the
// host keeps running, neighbors register).
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export default {
  id: "slowsetup",
  async setup(_ctx) {
    await sleep(6000);
    return async () => {};
  },
};
