#!/usr/bin/env python3
"""Manage the user's KU authenticator enrollment through KU's account API."""

import http.cookiejar
import json
from pathlib import Path
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from firefox_totp import current_code
from hendrix import PASSWORD

ORIGIN = 'https://mfa.ku.dk'
PREFIX = '/rest/user/api'
EVENT = 'Authenticators Management'


class Account:
    def __init__(self):
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.csrf = ''
        self.user_id = None
        self.tenant = ''
        self.process = None

    def request(self, path, method='GET', data=None):
        headers = {'Accept': 'application/json', 'X-Requested-With': 'XMLHttpRequest',
                   'Origin': ORIGIN, 'Referer': ORIGIN + '/account/'}
        if self.csrf:
            headers['X-CSRF-Token'] = self.csrf
        body = None
        if data is not None:
            body = json.dumps(data).encode()
            headers['Content-Type'] = 'application/json; charset=UTF-8'
        req = urllib.request.Request(ORIGIN + path, data=body,
                                     headers=headers, method=method)
        try:
            response = self.opener.open(req, timeout=15)
        except urllib.error.HTTPError as error:
            raise RuntimeError(f'KU account request failed with HTTP {error.code}.') from None
        with response:
            self.csrf = response.headers.get('X-CSRF-Token') or self.csrf
            raw = response.read()
        return json.loads(raw) if raw else None

    def authenticate(self):
        with self.opener.open(ORIGIN + '/account/', timeout=15) as response:
            self.csrf = response.headers.get('X-CSRF-Token') or ''
            response.read()
        query = urllib.parse.urlencode({'user_name': 'zlp616', 'event': EVENT,
                                        'force_wizard': 'false'})
        initial = self.request(PREFIX + '/auth?' + query)
        assert any(c.get('methods') == ['LDAP_PASSWORD:1', 'TOTP:1']
                   for c in initial.get('chains', [])), 'KU did not offer the Firefox TOTP login chain.'
        self.tenant = initial.get('tenant_name') or ''
        password = PASSWORD.read_text().rstrip('\n')
        first = self.step('LDAP_PASSWORD:1', password)
        del password
        if first.get('result', {}).get('status') in ('FAILED', 'STOP'):
            raise RuntimeError('KU rejected the saved account password.')
        self.process = first.get('logon_process_id')
        second = self.step('TOTP:1', current_code())
        result = second.get('result', {})
        session = second.get('session', {})
        if result.get('status') != 'OK' or not session.get('user_id'):
            raise RuntimeError('KU did not confirm the Firefox authenticator login.')
        username = (session.get('user_name') or session.get('user_login') or '').lower()
        if username and username.rsplit('\\', 1)[-1].split('@')[0] != 'zlp616':
            raise RuntimeError('KU returned an unexpected account identity.')
        self.user_id = session['user_id']
        return True

    def step(self, method, password):
        return self.request(PREFIX + '/auth', 'POST', {
            'event': EVENT, 'category_id': None,
            'tenant_name': self.tenant, 'user_name': 'zlp616',
            'method_id': method, 'logon_process_id': self.process,
            'data': {'password': password}})

    def templates(self):
        assert self.user_id
        doc = self.request(PREFIX + '/users/' + self.user_id + '/templates')
        if isinstance(doc, list):
            return doc
        if isinstance(doc, dict):
            for key in ('templates', 'items', 'data'):
                if isinstance(doc.get(key), list):
                    return doc[key]
        raise RuntimeError('KU returned an unexpected authenticator list.')

    def remove_smartphone(self):
        before = self.templates()
        protected = {item['id'] for item in before
                     if item.get('id') and item.get('method_id') != 'SMARTPHONE:1'}
        targets = [item for item in before
                   if item.get('id') and item.get('method_id') == 'SMARTPHONE:1']
        assert any(item.get('id') and item.get('method_id') == 'TOTP:1'
                   for item in before), 'The Firefox authenticator must remain enrolled.'
        for item in targets:
            path = PREFIX + '/users/' + self.user_id + '/templates/' + item['id']
            self.request(path, 'DELETE')
        after = self.templates()
        assert not any(item.get('id') and item.get('method_id') == 'SMARTPHONE:1'
                       for item in after), 'Smartphone-App is still enrolled.'
        assert protected <= {item.get('id') for item in after}, 'Another authenticator unexpectedly changed.'
        return len(targets), after


def summary(items):
    return [{'method': item.get('method_id'), 'comment': item.get('comment', ''),
             'enrolled': bool(item.get('id'))} for item in items]


def main():
    account = Account()
    account.authenticate()
    print(json.dumps({'status': 'firefox-login-verified',
                      'authenticators': summary(account.templates())}), flush=True)
    # Keep session cookies in memory while the next specific action is reviewed.
    for action in sys.stdin:
        action = action.strip()
        if action == 'remove-smartphone':
            count, items = account.remove_smartphone()
            print(json.dumps({'status': 'smartphone-removed', 'removed': count,
                              'authenticators': summary(items)}), flush=True)
        elif action == 'inspect':
            print(json.dumps({'status': 'inspected',
                              'authenticators': summary(account.templates())}), flush=True)
        elif action == 'exit':
            break
        else:
            print(json.dumps({'status': 'unknown-action'}), flush=True)


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, AssertionError) as error:
        print(json.dumps({'status': 'error', 'message': str(error)}), flush=True)
        sys.exit(1)
