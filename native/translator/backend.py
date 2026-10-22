#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Private, one-request JSON worker. Text and credentials travel over pipes only."""
import ast
import configparser
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import unicodedata
import urllib.error
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parent / 'vendor'))
API = 'https://api-free.deepl.com'
MAX_TEXT = 24000  # leave room for JSON escaping within DeepL's 128 KiB request limit
LANGUAGES = {
    'en': 'English', 'cs': 'Czech', 'sr': 'Serbian', 'ja': 'Japanese', 'ko': 'Korean',
    'de': 'German', 'fr': 'French', 'es': 'Spanish', 'it': 'Italian', 'pt': 'Portuguese',
    'pl': 'Polish', 'sk': 'Slovak', 'uk': 'Ukrainian', 'ru': 'Russian', 'bg': 'Bulgarian',
    'el': 'Greek', 'ar': 'Arabic', 'he': 'Hebrew', 'zh': 'Chinese', 'hi': 'Hindi',
    'th': 'Thai', 'vi': 'Vietnamese', 'tr': 'Turkish', 'nl': 'Dutch', 'hu': 'Hungarian',
    'ro': 'Romanian', 'da': 'Danish', 'fi': 'Finnish', 'sv': 'Swedish', 'nb': 'Norwegian',
    'id': 'Indonesian', 'lt': 'Lithuanian', 'lv': 'Latvian', 'et': 'Estonian',
}
LAYOUTS = {'us': 'en', 'gb': 'en', 'cz': 'cs', 'rs': 'sr', 'jp': 'ja', 'kr': 'ko',
           'cn': 'zh', 'tw': 'zh', 'gr': 'el', 'il': 'he', 'in': 'hi', 'ua': 'uk'}
METHODS = {'anthy': 'ja', 'mozc': 'ja', 'skk': 'ja', 'hangul': 'ko',
           'pinyin': 'zh', 'rime': 'zh', 'chewing': 'zh', 'unikey': 'vi'}


def config_dir():
    return Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'dome/translator'


def key_path():
    return config_dir() / 'deepl-key'


def api_key():
    key = os.environ.get('DEEPL_API_KEY', '').strip()
    if not key:
        try:
            p = key_path()
            fd = os.open(p, os.O_RDONLY | os.O_NOFOLLOW)
            with os.fdopen(fd) as f:
                s = os.fstat(f.fileno())
                if s.st_uid != os.getuid() or stat.S_IMODE(s.st_mode) & 0o077:
                    raise ValueError('Saved API key must be private (mode 0600).')
                key = f.read(1024).strip()
        except FileNotFoundError:
            pass
    if not key:
        raise ValueError('Set your DeepL API Free key in Settings, or DEEPL_API_KEY.')
    if not key.endswith(':fx') or '\n' in key or '\r' in key:
        raise ValueError('A DeepL API Free key ending in :fx is required.')
    return key


def save_key(key):
    key = key.strip()
    if key and (not key.endswith(':fx') or any(c.isspace() for c in key)):
        raise ValueError('Enter a DeepL API Free key ending in :fx.')
    directory = config_dir()
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    if not key:
        key_path().unlink(missing_ok=True)
        return
    fd, temporary = tempfile.mkstemp(prefix='.key-', dir=directory)
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(key + '\n')
            f.flush()
            os.fsync(f.fileno())
        os.replace(temporary, key_path())
    finally:
        Path(temporary).unlink(missing_ok=True)


def request(path, body=None, key=None):
    headers = {'Authorization': 'DeepL-Auth-Key ' + (key or api_key()),
               'Content-Type': 'application/json', 'User-Agent': 'DomeD-Translator/1.0'}
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode()
    if data is not None and len(data) > 128 * 1024:
        raise ValueError('Text is too long for a DeepL request.')
    req = urllib.request.Request(API + path, data=data, headers=headers)
    try:
        # No redirect of credentials, and no automatic replay of billable POSTs.
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, *args, **kwargs):
                return None
        with urllib.request.build_opener(NoRedirect).open(req, timeout=30) as reply:
            payload = reply.read(2 * 1024 * 1024 + 1)
            if len(payload) > 2 * 1024 * 1024:
                raise ValueError('DeepL response is too large.')
            return json.loads(payload)
    except urllib.error.HTTPError as e:
        messages = {400: 'DeepL rejected the language pair or request.',
                    403: 'DeepL rejected the API key. Check Settings.',
                    413: 'Text is too long for DeepL.', 429: 'DeepL is busy. Try again later.',
                    456: 'DeepL API Free quota is exhausted.'}
        message = messages.get(e.code, f'DeepL returned HTTP {e.code}.')
        e.close()
        raise ValueError(message) from None
    except (urllib.error.URLError, TimeoutError, OSError):
        raise ValueError('Cannot reach DeepL. Check your network and try again.') from None


def dbus(method, *args):
    p = subprocess.run(['gdbus', 'call', '--session', '--dest', 'org.fcitx.Fcitx5',
                        '--object-path', '/controller', '--method',
                        'org.fcitx.Fcitx.Controller1.' + method, *args],
                       capture_output=True, text=True, timeout=3)
    if p.returncode:
        raise ValueError('Fcitx is unavailable')
    return ast.literal_eval(p.stdout)


def configured_methods():
    try:
        group = dbus('CurrentInputMethodGroup')[0]
        return [x[0] for x in dbus('InputMethodGroupInfo', group)[1]]
    except (ValueError, SyntaxError, OSError, subprocess.TimeoutExpired):
        base = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config'))
        for path in [base / 'dome/fcitx5/profile', base / 'fcitx5/profile']:
            if not path.exists():
                continue
            p = configparser.ConfigParser(interpolation=None)
            p.read(path)
            group = p.get('GroupOrder', '0', fallback='Default')
            groups = [s for s in p.sections() if re.fullmatch(r'Groups/\d+', s)]
            prefix = next((s for s in groups if p.get(s, 'Name', fallback='') == group),
                          groups[0] if groups else 'Groups/0')
            return [p.get(s, 'Name') for s in p.sections()
                    if s.startswith(prefix + '/Items/') and p.has_option(s, 'Name')]
        return ['keyboard-us']


def language_for_method(method):
    if method.startswith('keyboard-'):
        layout = method.split('-')[1]
        return LAYOUTS.get(layout, layout)
    if method in METHODS:
        return METHODS[method]
    for base in ['/usr/share/fcitx5/inputmethod', '/usr/local/share/fcitx5/inputmethod']:
        path = Path(base) / (Path(method).name + '.conf')
        if path.is_file():
            p = configparser.ConfigParser(interpolation=None, strict=False)
            p.read(path)
            lang = p.get('InputMethod', 'LangCode', fallback='').split('_')[0].split('-')[0]
            if lang:
                return lang
    return None


def local_languages():
    result = []
    for method in configured_methods():
        code = language_for_method(method)
        if code and not any(x['code'] == code for x in result):
            result.append({'code': code, 'name': LANGUAGES.get(code, code.upper())})
    return result


def capabilities():
    data = request('/v3/languages?resource=translate_text')
    return data if isinstance(data, list) else data['languages']


def resolve(code, languages, target):
    if not code and not target:
        return None
    direction = 'usable_as_target' if target else 'usable_as_source'
    candidates = [x['lang'] for x in languages if x.get(direction) and
                  x['lang'].lower().split('-')[0] == code.lower()]
    # US input layout uses US English; regional variants do not add input languages.
    preferred = {'en': 'en-US', 'pt': 'pt-PT', 'zh': 'zh-HANS'}.get(code, code)
    return next((c for c in candidates if c.lower() == preferred.lower()),
                candidates[0] if candidates else None)


def icu_latin(text):
    """ICU fallback for configured scripts beyond Japanese/Korean."""
    lib = ctypes.CDLL(ctypes.util.find_library('icui18n'))
    major = re.search(r'\.so\.(\d+)', lib._name)
    suffix = '_' + major[1] if major else ''
    op = getattr(lib, 'utrans_openU' + suffix)
    op.argtypes = [ctypes.c_void_p, ctypes.c_int32, ctypes.c_int, ctypes.c_void_p,
                   ctypes.c_int32, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
    op.restype = ctypes.c_void_p
    trans = getattr(lib, 'utrans_transUChars' + suffix)
    trans.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int32),
                     ctypes.c_int32, ctypes.c_int32, ctypes.POINTER(ctypes.c_int32),
                     ctypes.POINTER(ctypes.c_int)]
    close = getattr(lib, 'utrans_close' + suffix)
    close.argtypes = [ctypes.c_void_p]
    ident = 'Any-Latin; Latin-ASCII'.encode('utf-16-le')
    status = ctypes.c_int(0)
    handle = op(ident, len(ident) // 2, 0, None, 0, None, ctypes.byref(status))
    if not handle or status.value > 0:
        raise ValueError('Transcription is unavailable for this script.')
    try:
        raw = text.encode('utf-16-le')
        capacity = max(256, len(raw) * 16)
        buffer = ctypes.create_string_buffer(capacity * 2)
        buffer.raw = raw + bytes(len(buffer) - len(raw))
        length = ctypes.c_int32(len(raw) // 2)
        limit = ctypes.c_int32(length.value)
        trans(handle, buffer, ctypes.byref(length), capacity, 0,
              ctypes.byref(limit), ctypes.byref(status))
        if status.value > 0:
            raise ValueError('Transcription failed for this text.')
        return buffer.raw[:length.value * 2].decode('utf-16-le')
    finally:
        close(handle)


def transcription(text, language):
    language = language.lower().split('-')[0]
    if language in {'en', 'cs', 'sr'} or not text:
        return ''
    if language == 'ja':
        import pykakasi
        # Kanji readings use the Japanese dictionary, never Chinese Han readings.
        parts = pykakasi.kakasi().convert(text)
        return ' '.join(p['hepburn'] for p in parts).strip()
    if language == 'ko':
        return icu_latin(text)
    scripts = [unicodedata.name(c, '') for c in text if c.isalpha()]
    if any(not ('LATIN' in c or 'CYRILLIC' in c) for c in scripts):
        return icu_latin(text)
    return ''


def run(task):
    action = task.get('action')
    if action == 'save-key':
        save_key(task.get('key', ''))
        return {'saved': True, 'environment_key': bool(os.environ.get('DEEPL_API_KEY'))}
    if action == 'languages':
        local = local_languages()
        result = {'languages': local, 'configured': [x['code'] for x in local], 'api_ready': False}
        try:
            api_key()
        except ValueError as e:
            result['notice'] = str(e)
            return result
        try:
            caps = capabilities()
        except ValueError as error:
            result['notice'] = str(error)
            return result
        for x in local:
            x['source'] = bool(resolve(x['code'], caps, False))
            x['target'] = bool(resolve(x['code'], caps, True))
        result['api_ready'] = True
        return result
    if action == 'translate':
        text = task.get('text', '')
        if not text.strip():
            raise ValueError('Enter or paste text first.')
        if len(text) > MAX_TEXT:
            raise ValueError(f'Text is too long (maximum {MAX_TEXT:,} characters).')
        configured = {x['code'] for x in local_languages()}
        source, target = task.get('source', ''), task['target']
        if target not in configured or (source and source not in configured):
            raise ValueError('Choose a language from the current system input group. Refresh languages if needed.')
        caps = capabilities()
        target_id, source_id = resolve(target, caps, True), resolve(source, caps, False)
        if not target_id or (source and not source_id):
            raise ValueError('DeepL API Free does not currently support this language pair.')
        body = {'text': [text], 'target_lang': target_id, 'preserve_formatting': True}
        if source_id:
            body['source_lang'] = source_id
        result = request('/v2/translate', body)['translations'][0]
        detected = result.get('detected_source_language', source).lower().split('-')[0]
        output = {'text': result['text'], 'detected': detected, 'source_reading': '', 'reading': ''}
        # A transcription failure must never discard a successful, billable translation.
        warnings = []
        for field, original, lang in [('reading', result['text'], target),
                                      ('source_reading', text, source or detected)]:
            try:
                output[field] = transcription(original, lang)
            except Exception:
                warnings.append('Transcription unavailable; original script is preserved.')
        output['notice'] = ' '.join(dict.fromkeys(warnings))
        return output
    if action == 'copy-primary':
        try:
            p = subprocess.run(['wl-copy', '--primary', '--type', 'text/plain;charset=utf-8'],
                               input=task.get('text', '').encode('utf-8'),
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3)
        except FileNotFoundError:
            raise ValueError('Install wl-clipboard for Wayland selection support.') from None
        if p.returncode:
            raise ValueError('Cannot copy to primary selection.')
        return {'copied_primary': True}
    if action == 'paste':
        if not os.environ.get('WAYLAND_DISPLAY'):
            raise ValueError('Native X11 selection must be handled by FLTK.')
        args = ['wl-paste', '--no-newline', '--type', 'text']
        if task.get('primary'):
            args.append('--primary')
        try:
            p = subprocess.run(args, capture_output=True, timeout=3)
        except FileNotFoundError:
            raise ValueError('Install wl-clipboard for Wayland selection support.') from None
        if p.returncode:
            raise ValueError('The primary selection is empty or unavailable.' if task.get('primary')
                             else 'The clipboard has no text.')
        text = p.stdout.decode('utf-8', errors='replace')
        if len(text) > MAX_TEXT:
            raise ValueError('Selection is too long.')
        return {'paste': text}
    raise ValueError('Unknown translator operation.')


if __name__ == '__main__':
    try:
        task = json.loads(sys.stdin.buffer.read(256 * 1024))
        result = run(task)
        print(json.dumps({'ok': True, **result}, ensure_ascii=False))
    except Exception as error:
        # Never reflect server response bodies, headers, keys or translated text in errors.
        safe = str(error) if isinstance(error, ValueError) and not isinstance(error, json.JSONDecodeError) else 'Translator operation failed. Check installation and settings.'
        print(json.dumps({'ok': False, 'error': safe}, ensure_ascii=False))
