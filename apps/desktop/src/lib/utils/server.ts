export { defaultRetryPolicy, nativeProtocolForEntry, routeProtocolFor, routeNeedsConversion, apiBaseUrl, proxySupportedEntry, buildRouteTarget, buildSingleEntryRoute, advertisedProxyAddress, reorderItems, mergeRouteTargets } from "@aipass/schemas";

import type { ProxyRouteConfig } from "@aipass/schemas";

export function prepareRouteSave(routes: readonly ProxyRouteConfig[], route: ProxyRouteConfig) {
  const created = !routes.some(item => item.id === route.id);
  return { created, routes: created ? [...routes, route] : routes.map(item => item.id === route.id ? route : item) };
}
