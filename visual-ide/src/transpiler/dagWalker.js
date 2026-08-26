/**
 * dagWalker.js
 * DAG(방향 비순환 그래프)를 위상 정렬하여 순회하는 유틸리티
 */

/**
 * in-degree가 0인 루트 노드를 찾습니다.
 * @param {Array} nodes
 * @param {Array} edges
 * @returns {Array} 루트 노드 배열
 */
export function findRootNodes(nodes, edges) {
  const targetIds = new Set(edges.map(e => e.target));
  return nodes.filter(n => !targetIds.has(n.id) && n.type !== 'comment' && n.type !== 'container');
}

/**
 * Kahn's 알고리즘을 사용하여 노드를 위상 정렬합니다.
 * @param {Array} nodes
 * @param {Array} edges
 * @returns {Array} 위상 정렬된 노드 배열
 */
export function topologicalSort(nodes, edges) {
  const validNodes = nodes.filter(n => n.type !== 'comment' && n.type !== 'container');
  const nodeMap = new Map(validNodes.map(n => [n.id, n]));

  // in-degree 계산
  const inDegree = new Map(validNodes.map(n => [n.id, 0]));
  const adjacency = new Map(validNodes.map(n => [n.id, []]));

  edges.forEach(e => {
    if (nodeMap.has(e.source) && nodeMap.has(e.target)) {
      inDegree.set(e.target, (inDegree.get(e.target) || 0) + 1);
      adjacency.get(e.source)?.push(e.target);
    }
  });

  // in-degree 0인 노드부터 큐에 추가
  const queue = [];
  inDegree.forEach((degree, nodeId) => {
    if (degree === 0) queue.push(nodeId);
  });

  const sorted = [];
  while (queue.length > 0) {
    const current = queue.shift();
    const node = nodeMap.get(current);
    if (node) sorted.push(node);

    const neighbors = adjacency.get(current) || [];
    neighbors.forEach(neighborId => {
      const newDegree = (inDegree.get(neighborId) || 0) - 1;
      inDegree.set(neighborId, newDegree);
      if (newDegree === 0) queue.push(neighborId);
    });
  }

  return sorted;
}

/**
 * 특정 루트 노드에서 시작하는 파이프라인 체인을 순서대로 반환합니다.
 * @param {string} rootId - 시작 노드 ID
 * @param {Array} nodes
 * @param {Array} edges
 * @returns {Array} 순서대로 정렬된 노드 배열
 */
export function resolvePipelineChain(rootId, nodes, edges) {
  const nodeMap = new Map(nodes.map(n => [n.id, n]));
  const result = [];
  const visited = new Set();

  function dfs(nodeId) {
    if (visited.has(nodeId)) return;
    visited.add(nodeId);
    const node = nodeMap.get(nodeId);
    if (node) result.push(node);
    // 현재 노드를 source로 가지는 edge들의 target을 찾아 계속 탐색
    const outgoing = edges.filter(e => e.source === nodeId);
    outgoing.forEach(e => dfs(e.target));
  }

  dfs(rootId);
  return result;
}

/**
 * 특정 노드의 직접 업스트림 노드를 반환합니다.
 * @param {string} nodeId
 * @param {Array} nodes
 * @param {Array} edges
 * @returns {Array} 업스트림 노드 배열
 */
export function getUpstreamNodes(nodeId, nodes, edges) {
  const incomingEdges = edges.filter(e => e.target === nodeId);
  return incomingEdges.map(e => nodes.find(n => n.id === e.source)).filter(Boolean);
}
