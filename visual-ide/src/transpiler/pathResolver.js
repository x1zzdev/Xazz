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

/**
 * 안전한 저장소 데이터 경로인지 검증한다. (경로 트래버설/임의 파일 로드 방지)
 * - `..` 세그먼트, 선행 `/` 또는 `\`, Windows 드라이브 문자, URL 스킴 거부
 * - 허용 접두어: `visual-ide/`, `examples/`
 * @param {string} path
 * @returns {boolean}
 */
export function isSafeRepoPath(path) {
  if (typeof path !== 'string' || path.length === 0) return false;
  if (/^[A-Za-z]:[\\/]/.test(path)) return false;
  if (path.startsWith('/') || path.startsWith('\\')) return false;
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(path)) return false; // URL scheme (e.g. file://)
  if (path.split(/[\\/]/).includes('..')) return false;
  return path.startsWith('visual-ide/') || path.startsWith('examples/');
}

// 브라우저에서는 파일시스템 접근 불가 — resolvePath는 경로 정규화만 수행.
export function resolvePath(path) {
  if (!path || typeof path !== 'string') {
    throw new Error(`[XAZZ PATH ERROR] Path must be a non-empty string, got: ${JSON.stringify(path)}`);
  }
  if (isSafeRepoPath(path)) {
    return path;
  }
  // 절대 경로 / 드라이브 문자 / URL 스킴 / 트래버설은 허용하지 않는다.
  if (path.startsWith('/') || path.startsWith('\\') || /^[A-Za-z]:[\\/]/.test(path) || /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(path) || path.split(/[\\/]/).includes('..')) {
    throw new Error(`[XAZZ PATH ERROR] 안전하지 않은 경로입니다 (트래버설/절대 경로 불허): ${path}`);
  }
  return `visual-ide/data/${path.replace(/^\.\//, '')}`;
}

export function isPathResolved(path) {
  return isSafeRepoPath(path);
}