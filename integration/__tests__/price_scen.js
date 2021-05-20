const {
  buildScenarios,
} = require('../util/scenario');

let prices_scen_info = {
  tokens: [
    { token: "zrx", balances: { ashley: 1000 } }
  ],
  prices: {
    prices: {}
  }
};

let pricePayloads = {
  "ZRX": {
    price: "0.5994535000000001",
    payload: "0x00000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000060124a7000000000000000000000000000000000000000000000000000000000000000c0000000000000000000000000000000000000000000000000000000000009259d0000000000000000000000000000000000000000000000000000000000000006707269636573000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000035a52580000000000000000000000000000000000000000000000000000000000",
    signature: "0x88a17e64f1f3417c826f253e3e61fbafb735a476aad6ebc3352cc9cb65667b5f832928d31309eef0c0030f17a2ffbce71bb783cebf8b397ceaee5103fc3c8db9000000000000000000000000000000000000000000000000000000000000001c",
  }
};

async function postPrice({ chain }) {
  await chain.postPrice(pricePayloads.ZRX.payload, pricePayloads.ZRX.signature, false);
}

buildScenarios('Price Scenarios', prices_scen_info, { beforeEach: postPrice }, [
  {
    name: "Load Price from Storage",
    scenario: async ({ chain, zrx }) => {
      expect(await zrx.getPrice()).toEqual(0.599453);
    }
  },
  {
    name: "Prices from RPC",
    scenario: async ({ api }) => {
      expect((await api.rpc.gateway.price("ZRX")).toJSON()).toEqual("599453");
    }
  },
  {
    name: "Post Price Tx",
    skip: true
  }
]);
