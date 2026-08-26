/**
 * pathResolver.js — Xazz 경로 정규화/리졸버
 *
 * Xazz 백엔드는 @data alias 대신 저장소 내 상대 경로(visual-ide/data/...)를 사용한다.
 * 여기서는 UI 표시용 경로 정규화와 컬럼명 sanitize 헬퍼를 제공한다.
 */

// ─── 컬럼 이름 sanitize ─────────────────────────────────────────────────────
// Xazz DSL 식별자에 허용되지 않는 문자(괄호, 공백, 특수문자 등)를 _로 치환. 한글 허용.
export function sanitizeFieldName(name) {
  return String(name).replace(/[^a-zA-Z0-9_\uAC00-\uD7AF]/g, '_');
}

// ─── 경로 헬퍼 ───────────────────────────────────────────────────────────────
export function isAliasPath(path) {
  return typeof path === 'string' && path.startsWith('@');
}

export function getRegisteredAliases() {
  return ['@data', '@assets'];
}

// 브라우저에서는 파일시스템 접근 불가 — resolvePath는 경로 정규화만 수행.
export function resolvePath(path) {
  if (!path || typeof path !== 'string') {
    throw new Error(`[XAZZ PATH ERROR] Path must be a non-empty string, got: ${JSON.stringify(path)}`);
  }
  // 상대/절대 경로는 그대로
  if (path.startsWith('visual-ide/') || path.startsWith('examples/') || /^[A-Za-z]:[\\\/]/.test(path) || path.startsWith('/')) {
    return path;
  }
  return `visual-ide/data/${path.replace(/^\.\//, '')}`;
}

export function isPathResolved(path) {
  return typeof path === 'string' && (path.startsWith('visual-ide/') || path.startsWith('examples/'));
}