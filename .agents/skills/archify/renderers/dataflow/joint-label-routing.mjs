import {
  automaticPortRhythmBridge, collectAmbiguousCorridors, collectBorderRuns,
  collectRouteRhythmIssues, frameBorderSegments, normalizeRoutePoints,
  properSegmentIntersection, rectsOverlap, routeHonorsEndpointSides,
  segmentIntersectsRect, segmentRectClearanceWithin,
} from '../shared/geometry.mjs';
import { shortestOrthogonalGridRoute } from '../shared/route-quality.mjs';

const segments = points => points.slice(1).map((end, index) => ({ start: points[index], end }));
const related = (a, b) => [a.from, a.to].some(id => id === b.from || id === b.to);
const length = points => segments(points).reduce((sum, { start, end }) => sum
  + Math.abs(start[0] - end[0]) + Math.abs(start[1] - end[1]), 0);
const distance = (points, rect, limit = 36) => Math.min(...segments(points)
  .map(segment => segmentRectClearanceWithin(segment, rect, limit)));
const masks = (points, rect) => distance(points, rect, 4) + 0.0001 < 4;
const pinned = flow => flow.via || (flow.route && flow.route !== 'auto')
  || ['fromSide', 'toSide', 'channelX', 'channelY', 'labelAt', 'labelDx', 'labelDy', 'labelSegment']
    .some(key => flow[key] !== undefined);
const crossingCount = (a, b) => segments(a).reduce((sum, s) => sum + segments(b)
  .filter(t => properSegmentIntersection(s.start, s.end, t.start, t.end)).length, 0);
// One complete label, its own bridge, and the few routes blocking its slot
// are a single proposal. Nothing is applied until all the geometry clears.
// Keep all endpoint coordinates and every other label fixed.
export function repairLabelRoutes({ labels, routes, nodes, titles, frames, viewBox,
  placementBottom, sideFor, maximumSearches = 32, metrics = {} }) {
  if (nodes.length + titles.length + labels.length - 1 > 80) {
    Object.assign(metrics, { searches: 0, maximumSearches, changedLabels: 0, changedRoutes: 0 });
    return { labels, routes };
  }
  let currentLabels = labels, currentRoutes = routes;
  let searches = 0;
  const rectangles = [...nodes, ...titles];
  const borderSegments = frames.flatMap(frameBorderSegments);
  const clearRect = (rect, index) => rect.x >= 0 && rect.y >= 0
    && rect.x + rect.width <= viewBox[0] && rect.y + rect.height <= placementBottom
    && !rectangles.some(node => rectsOverlap(rect, node, 2))
    && !currentLabels.some((other, i) => i !== index && rectsOverlap(rect, other, 2));
  const intrudes = (route, points) => nodes.some(node => segments(points).some((segment, index) => (
    !((node.id === route.relation.from && index === 0)
      || (node.id === route.relation.to && index === points.length - 2))
    && segmentIntersectsRect(segment, node, 2)
  )));
  const routeClear = (route, points, nextLabels, nextRoutes, bottom) => {
    const { fromSide, toSide } = sideFor(route.relation);
    return routeHonorsEndpointSides(points, fromSide, toSide)
      && points.every(([x, y]) => x >= 0 && y >= 0 && x <= viewBox[0] && y <= bottom)
      && !intrudes(route, points)
      && !collectRouteRhythmIssues({ routedRelations: [{ points }] }).length
      && !collectBorderRuns({ routedRelations: [{ points }], frames }).some(hit => hit.side !== 'bottom')
      && !nextLabels.some(rect => rect.relationIndex !== route.relationIndex && masks(points, rect))
      && nextLabels.filter(rect => rect.relationIndex === route.relationIndex).every(rect => distance(points, rect) <= 36)
      && nextRoutes.every(other => other.relationIndex === route.relationIndex
        || !pinned(other.relation) || crossingCount(points, other.points) === 0)
      && nextRoutes.every(other => other.relationIndex === route.relationIndex
        || !collectAmbiguousCorridors({ routedRelations: [{ ...route, points }, other],
          includeSharedEndpoints: () => true }).length
        || collectAmbiguousCorridors({ routedRelations: [route, currentRoutes.find(r => r.relationIndex === other.relationIndex)],
          includeSharedEndpoints: () => true }).length > 0);
  };
  for (const [labelIndex, label] of currentLabels.entries()) {
    if (searches >= maximumSearches) break;
    const own = currentRoutes.find(route => route.relationIndex === label.relationIndex);
    if (!own || pinned(own.relation)
      || (!currentRoutes.some(route => route !== own && masks(route.points, label))
        && !nodes.some(node => rectsOverlap(label, node, -2)))) continue;
    const { fromSide, toSide } = sideFor(own.relation);
    const ownOptions = new Map([[JSON.stringify(own.points), own.points]]);
    for (const endpointStubPx of [24, 16]) automaticPortRhythmBridge(own.points[0], own.points.at(-1), fromSide, toSide, {
      endpointStubPx, accept: points => { ownOptions.set(JSON.stringify(points), points); return false; },
    });
    const xs = new Set([label.x]), ys = new Set([label.y]);
    for (const points of ownOptions.values()) for (const { start, end } of segments(points)) {
      for (const x of [start[0], end[0], (start[0] + end[0]) / 2]) xs.add(x - label.width / 2);
      for (const y of [start[1], end[1], (start[1] + end[1]) / 2]) {
        ys.add(y - label.height - 4); ys.add(y + 4);
      }
    }
    const neighbours = [...rectangles, ...currentLabels.filter((_, index) => index !== labelIndex)];
    for (const rect of neighbours) {
      xs.add(rect.x - label.width - 2); xs.add(rect.x + rect.width + 2);
      ys.add(rect.y - label.height - 2); ys.add(rect.y + rect.height + 2);
    }
    for (const route of currentRoutes) for (const { start, end } of segments(route.points)) {
      if (start[0] === end[0]) { xs.add(start[0] - label.width - 8); xs.add(start[0] + 8); }
      if (start[1] === end[1]) { ys.add(start[1] - label.height - 8); ys.add(start[1] + 8); }
    }
    const xPoints = [...ownOptions.values()].flatMap(points => points.map(point => point[0]));
    const yPoints = [...ownOptions.values()].flatMap(points => points.map(point => point[1]));
    const candidates = [];
    for (const x of xs) {
      if (x + label.width < Math.min(...xPoints) - 36 || x > Math.max(...xPoints) + 36) continue;
      for (const y of ys) {
        if (y + label.height < Math.min(...yPoints) - 36 || y > Math.max(...yPoints) + 36) continue;
        const rect = { ...label, x, y, lx: x + label.width / 2, ly: y + (label.ly - label.y) };
        if (!clearRect(rect, labelIndex)) continue;
        for (const points of ownOptions.values()) {
          const ownDistance = distance(points, rect);
          if (ownDistance < 4 || ownDistance > 36) continue;
          const nextLabels = currentLabels.map((other, index) => index === labelIndex ? rect : other);
          if (!routeClear(own, points, nextLabels, currentRoutes, placementBottom)) continue;
          const blocked = currentRoutes.filter(route => route !== own && masks(route.points, rect));
          if (blocked.length > 4 || blocked.some(route => pinned(route.relation))) continue;
          const stableDistance = Math.min(...currentRoutes.filter(route => route !== own && !blocked.includes(route))
            .map(route => distance(route.points, rect)));
          if (ownDistance > stableDistance + 0.0001) continue;
          candidates.push({ rect, points, blocked, ownDistance, move: Math.hypot(x - label.x, y - label.y) });
        }
      }
    }
    candidates.sort((a, b) => a.blocked.length - b.blocked.length || a.ownDistance - b.ownDistance || a.move - b.move);
    const chosen = [];
    // Avoid spending every search on virtually identical neighbouring slots.
    for (const c of candidates) {
      if (chosen.some(other => Math.hypot(c.rect.x - other.rect.x, c.rect.y - other.rect.y) < 8
        && JSON.stringify(c.points) === JSON.stringify(other.points))) continue;
      chosen.push(c); if (chosen.length >= 16) break;
    }
    let accepted = null;
    for (const candidate of chosen) {
      if (searches >= maximumSearches) break;
      const nextLabels = currentLabels.map((other, index) => index === labelIndex ? candidate.rect : other);
      const nextRoutes = currentRoutes.map(route => route === own ? { ...route, points: candidate.points } : route);
      // Free the longest occupied corridor first. Each subsequent route sees
      // the accepted proposal; all unchanged routes remain obstacles.
      const blocked = [...candidate.blocked].sort((a, b) => length(b.points) - length(a.points));
      let failed = false;
      for (const old of blocked) {
        if (searches >= maximumSearches) { failed = true; break; }
        searches++;
        const { fromSide, toSide } = sideFor(old.relation);
        const obstacles = [
          ...nodes.map(node => [old.relation.from, old.relation.to].includes(node.id) ? node : {
            ...node, x: node.x - 12, y: node.y - 12, width: node.width + 24, height: node.height + 24,
          }), ...titles,
          ...nextLabels.filter(rect => rect.relationIndex !== old.relationIndex).map(rect => ({
            ...rect, x: rect.x - 2, y: rect.y - 2, width: rect.width + 4, height: rect.height + 4,
          })),
        ];
        const found = shortestOrthogonalGridRoute({ start: old.points[0], end: old.points.at(-1),
          points: [old.points[0], old.points.at(-1)], fromSide, toSide, obstacles,
          clearance: 2, maximumObstacleCount: 80, endpointStubPx: 24, maximumGridNodes: 4096,
          avoidedSegments: nextRoutes.filter(other => other.relationIndex !== old.relationIndex
            && !related(old.relation, other.relation)).flatMap(other => segments(other.points)),
          allowAvoidedCrossings: true, minimumAvoidedOverlapPx: 8, routeSeparationPx: 8,
          minimumSegmentPx: 16, bendPenaltyPx: 48, borderSegments,
          accept: points => routeClear(old, points, nextLabels, nextRoutes, placementBottom + label.height),
        });
        const points = found && normalizeRoutePoints(found.points);
        if (!points) { failed = true; break; }
        nextRoutes[nextRoutes.findIndex(route => route.relationIndex === old.relationIndex)] = { ...old, points };
      }
      if (failed) continue;
      const foreignDistance = Math.min(...nextRoutes.filter(route => route.relationIndex !== own.relationIndex)
        .map(route => distance(route.points, candidate.rect)));
      // The original label is invalid. A collision-free repair may need a longer
      // corridor; never rank that invalid geometry above a valid proposal.
      if (candidate.ownDistance > foreignDistance + 0.0001) continue;
      accepted = { labels: nextLabels, routes: nextRoutes }; break;
    }
    if (accepted) { currentLabels = accepted.labels; currentRoutes = accepted.routes; }
  }
  Object.assign(metrics, { searches, maximumSearches, changedLabels: currentLabels.filter((rect, i) => rect !== labels[i]).length,
    changedRoutes: currentRoutes.filter((route, i) => route !== routes[i]).length });
  return { labels: currentLabels, routes: currentRoutes };
}
