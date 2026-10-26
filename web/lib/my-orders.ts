// Orders opened in this browser, so "Your orders" works without connecting a wallet. Kept in
// localStorage only: a convenience, not a record (other devices and cleared storage miss it).

const KEY = "zec-ssl:my-orders";
const MAX = 50;

export function myOrderIds(): string[] {
  try {
    const ids = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(ids) ? ids.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

export function rememberOrder(id: string) {
  try {
    const ids = [id, ...myOrderIds().filter((x) => x !== id)].slice(0, MAX);
    localStorage.setItem(KEY, JSON.stringify(ids));
  } catch {
    // Private windows and blocked storage: the order page link still works.
  }
}
