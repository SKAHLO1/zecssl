/** Block explorer links for the network a deployment runs on. */

export type Net = "testnet" | "mainnet";

export function netOf(nearNetwork: string | null | undefined): Net {
  return nearNetwork === "mainnet" ? "mainnet" : "testnet";
}

export const explorer = {
  zcashTx: (net: Net, txid: string) =>
    net === "testnet" ? `https://testnet.zecblock.com/tx/${txid}` : `https://blockchair.com/zcash/transaction/${txid}`,
  nearAccount: (net: Net, account: string) =>
    net === "testnet" ? `https://testnet.nearblocks.io/address/${account}` : `https://nearblocks.io/address/${account}`,
  nearTx: (net: Net, hash: string) => (net === "testnet" ? `https://testnet.nearblocks.io/txns/${hash}` : `https://nearblocks.io/txns/${hash}`),
};
