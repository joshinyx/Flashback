export type HotkeyAction = 'saveReplay' | 'record' | 'open';

const STORAGE_KEY = 'flashback.hotkeys';

const defaults: Record<HotkeyAction, string> = {
  saveReplay: 'Alt+F8',
  record: 'Alt+F9',
  open: 'Alt+F10'
};

function load(): Record<HotkeyAction, string> {
  if (typeof localStorage === 'undefined') return { ...defaults };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...defaults, ...JSON.parse(raw) };
  } catch {
    // localStorage corrupto o bloqueado
  }
  return { ...defaults };
}

export const hotkeys = $state<Record<HotkeyAction, string>>(load());

// Mientras se reasigna un atajo en Ajustes hay que soltar los atajos globales: si no,
// el SO se traga la combinación (RegisterHotKey la intercepta) y nunca llega al capturador.
export const capture = $state({ active: false });

export function setHotkey(action: HotkeyAction, accel: string) {
  hotkeys[action] = accel;
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(hotkeys));
    } catch {
      // sin persistencia disponible
    }
  }
}

const MODS = ['Control', 'Alt', 'Shift', 'Super'];
const modLabel: Record<string, string> = {
  Control: 'CTRL',
  Alt: 'ALT',
  Shift: 'SHIFT',
  Super: 'WIN'
};

export function isModifier(token: string): boolean {
  return MODS.includes(token);
}

export function hasMainKey(tokens: string[]): boolean {
  return tokens.some((t) => !isModifier(t));
}

// Etiquetas legibles para teclas que no tienen un carácter obvio.
const keyLabel: Record<string, string> = {
  Space: 'Espacio',
  Enter: 'Intro',
  Tab: 'Tab',
  Backspace: '⌫',
  Delete: 'Supr',
  Insert: 'Ins',
  Home: 'Inicio',
  End: 'Fin',
  PageUp: 'Re Pág',
  PageDown: 'Av Pág',
  PrintScreen: 'Impr Pant',
  ScrollLock: 'Bloq Despl',
  Pause: 'Pausa',
  CapsLock: 'Bloq Mayús',
  NumLock: 'Bloq Num',
  Up: '↑',
  Down: '↓',
  Left: '←',
  Right: '→',
  NumpadAdd: 'Num +',
  NumpadSubtract: 'Num −',
  NumpadMultiply: 'Num ×',
  NumpadDivide: 'Num ÷',
  NumpadDecimal: 'Num .',
  NumpadEnter: 'Num Intro',
  NumpadEqual: 'Num ='
};

// Tokens canónicos (los que entiende el plugin al unir con '+') → etiquetas para la UI.
export function labelTokens(accel: string): string[] {
  return accel.split('+').map((t) => {
    if (modLabel[t]) return modLabel[t];
    if (keyLabel[t]) return keyLabel[t];
    const num = t.match(/^Numpad(\d)$/);
    if (num) return `Num ${num[1]}`;
    return t.toUpperCase();
  });
}

export function labelFor(accel: string): string {
  return labelTokens(accel).join(' + ');
}

// code del DOM → token canónico que el parser de global-hotkey acepta. No se incluye
// Escape: se reserva para cancelar la reasignación. Las teclas internacionales
// (IntlBackslash, IntlRo, IntlYen) no las soporta el plugin, así que quedan fuera.
const CODE_TOKEN: Record<string, string> = {
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Space: 'Space',
  Enter: 'Enter',
  Tab: 'Tab',
  Backspace: 'Backspace',
  Delete: 'Delete',
  Insert: 'Insert',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  PrintScreen: 'PrintScreen',
  ScrollLock: 'ScrollLock',
  Pause: 'Pause',
  CapsLock: 'CapsLock',
  NumLock: 'NumLock',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  NumpadAdd: 'NumpadAdd',
  NumpadSubtract: 'NumpadSubtract',
  NumpadMultiply: 'NumpadMultiply',
  NumpadDivide: 'NumpadDivide',
  NumpadDecimal: 'NumpadDecimal',
  NumpadEnter: 'NumpadEnter',
  NumpadEqual: 'NumpadEqual'
};

function isModifierCode(code: string): boolean {
  return /^(Control|Shift|Alt|Meta)(Left|Right)$/.test(code);
}

function codeToToken(code: string): string | null {
  let m: RegExpMatchArray | null;
  if ((m = code.match(/^Key([A-Z])$/))) return m[1];
  if ((m = code.match(/^Digit(\d)$/))) return m[1];
  if ((m = code.match(/^Numpad(\d)$/))) return `Numpad${m[1]}`;
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code;
  return CODE_TOKEN[code] ?? null;
}

// virtual-key code de Windows → token canónico. En Windows el atajo global se registra
// con RegisterHotKey, que casa por VK y depende de la distribución del teclado. e.code es
// físico (US), así que las teclas OEM (las de puntuación) no disparaban en layouts no-US:
// se registraba el VK equivocado. Usamos el VK real del evento, que sí coincide con la
// tecla pulsada. El token elegido es el que global-hotkey vuelve a traducir a ese mismo VK.
const VK_TOKEN: Record<number, string> = (() => {
  const map: Record<number, string> = {
    0xbb: '=', 0xbc: ',', 0xbd: '-', 0xbe: '.', 0xba: ';', 0xbf: '/',
    0xc0: '`', 0xdb: '[', 0xdc: '\\', 0xdd: ']', 0xde: "'",
    0x08: 'Backspace', 0x09: 'Tab', 0x20: 'Space', 0x0d: 'Enter',
    0x14: 'CapsLock', 0x90: 'NumLock', 0x91: 'ScrollLock', 0x13: 'Pause',
    0x21: 'PageUp', 0x22: 'PageDown', 0x23: 'End', 0x24: 'Home',
    0x25: 'Left', 0x26: 'Up', 0x27: 'Right', 0x28: 'Down',
    0x2c: 'PrintScreen', 0x2d: 'Insert', 0x2e: 'Delete',
    0x6b: 'NumpadAdd', 0x6d: 'NumpadSubtract', 0x6a: 'NumpadMultiply',
    0x6f: 'NumpadDivide', 0x6e: 'NumpadDecimal'
  };
  for (let i = 0; i < 26; i++) map[0x41 + i] = String.fromCharCode(65 + i);
  for (let i = 0; i <= 9; i++) map[0x30 + i] = String(i);
  for (let i = 0; i <= 9; i++) map[0x60 + i] = `Numpad${i}`;
  for (let i = 1; i <= 24; i++) map[0x70 + (i - 1)] = `F${i}`;
  return map;
})();

function mainKeyFromEvent(e: KeyboardEvent): string | null {
  if (isModifierCode(e.code)) return null;
  const vk = e.keyCode;
  if (vk && VK_TOKEN[vk]) return VK_TOKEN[vk];
  return codeToToken(e.code);
}

// Una tecla principal (no modificador) que el plugin no sabe registrar: sirve para
// avisar en la UI en vez de descartarla en silencio.
export function eventHasUnsupportedKey(e: KeyboardEvent): boolean {
  if (isModifierCode(e.code)) return false;
  return mainKeyFromEvent(e) === null;
}

// Combinación a partir de un evento de teclado, máximo 2 tokens (1 modificador + 1
// tecla, o una sola tecla). Devuelve [] si solo hay modificadores sin tecla principal.
export function comboFromEvent(e: KeyboardEvent): string[] {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push('Control');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey) mods.push('Super');

  const main = mainKeyFromEvent(e);
  if (main) return mods.length ? [mods[0], main] : [main];
  return mods.slice(0, 2);
}
