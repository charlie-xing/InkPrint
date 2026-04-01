#!/usr/bin/env python3
"""
InkPrint IPP 端到端测试脚本
用法:
    python3 scripts/test_ipp.py <printer_ip> [port]
    python3 scripts/test_ipp.py 192.168.1.100
    python3 scripts/test_ipp.py 192.168.1.100 631
"""

import sys
import struct
import socket
import time
import os

def encode_string(tag: int, name: str, value: str) -> bytes:
    """Encode an IPP string attribute."""
    b = bytes([tag])
    b += struct.pack('>H', len(name)) + name.encode()
    b += struct.pack('>H', len(value)) + value.encode()
    return b

def build_ipp_request(operation: int, request_id: int, attrs: list, doc_data: bytes = b'') -> bytes:
    """Build a raw IPP request."""
    buf = b''
    buf += bytes([1, 1])  # version 1.1
    buf += struct.pack('>H', operation)
    buf += struct.pack('>I', request_id)

    # operation-attributes group
    buf += bytes([0x01])
    buf += encode_string(0x47, 'attributes-charset', 'utf-8')
    buf += encode_string(0x48, 'attributes-natural-language', 'en')

    for attr in attrs:
        tag, name, value = attr
        if isinstance(value, str):
            buf += encode_string(tag, name, value)
        elif isinstance(value, int):
            buf += bytes([tag])
            buf += struct.pack('>H', len(name)) + name.encode()
            buf += struct.pack('>H', 4) + struct.pack('>i', value)

    buf += bytes([0x03])  # end-of-attributes
    buf += doc_data
    return buf

def parse_ipp_response(data: bytes) -> dict:
    """Parse minimal IPP response."""
    if len(data) < 8:
        return {'error': 'Response too short'}

    major, minor = data[0], data[1]
    status_code = struct.unpack('>H', data[2:4])[0]
    request_id = struct.unpack('>I', data[4:8])[0]

    status_names = {
        0x0000: 'successful-ok',
        0x0001: 'successful-ok-ignored-or-substituted',
        0x0400: 'client-error-bad-request',
        0x0404: 'client-error-not-possible',
        0x040A: 'client-error-document-format-not-supported',
        0x0500: 'server-error-internal-error',
        0x0501: 'server-error-operation-not-supported',
    }

    return {
        'version': f'{major}.{minor}',
        'status_code': status_code,
        'status_name': status_names.get(status_code, f'unknown-0x{status_code:04x}'),
        'request_id': request_id,
        'ok': status_code < 0x0100,
    }

def send_ipp(host: str, port: int, ipp_data: bytes, timeout: float = 10.0) -> bytes:
    """Send IPP request over HTTP and return response body."""
    http_request = (
        f'POST /ipp/print HTTP/1.1\r\n'
        f'Host: {host}:{port}\r\n'
        f'Content-Type: application/ipp\r\n'
        f'Content-Length: {len(ipp_data)}\r\n'
        f'Connection: close\r\n'
        f'\r\n'
    ).encode() + ipp_data

    with socket.create_connection((host, port), timeout=timeout) as s:
        s.sendall(http_request)

        response = b''
        while True:
            chunk = s.recv(65536)
            if not chunk:
                break
            response += chunk

    # Split HTTP headers from body
    header_end = response.find(b'\r\n\r\n')
    if header_end == -1:
        return response
    return response[header_end + 4:]

def test_get_printer_attributes(host: str, port: int) -> bool:
    """Test Get-Printer-Attributes operation."""
    print(f'\n=== Test: Get-Printer-Attributes ===')
    printer_uri = f'ipp://{host}:{port}/ipp/print'

    req = build_ipp_request(0x000B, 1, [
        (0x45, 'printer-uri', printer_uri),
    ])

    try:
        resp_bytes = send_ipp(host, port, req)
        resp = parse_ipp_response(resp_bytes)

        if resp['ok']:
            print(f'  ✓ Status: {resp["status_name"]} (0x{resp["status_code"]:04x})')
            return True
        else:
            print(f'  ✗ Status: {resp["status_name"]} (0x{resp["status_code"]:04x})')
            return False
    except Exception as e:
        print(f'  ✗ Error: {e}')
        return False

def test_validate_job(host: str, port: int) -> bool:
    """Test Validate-Job operation."""
    print(f'\n=== Test: Validate-Job ===')
    printer_uri = f'ipp://{host}:{port}/ipp/print'

    req = build_ipp_request(0x0004, 2, [
        (0x45, 'printer-uri', printer_uri),
        (0x49, 'document-format', 'application/pdf'),
    ])

    try:
        resp_bytes = send_ipp(host, port, req)
        resp = parse_ipp_response(resp_bytes)

        if resp['ok']:
            print(f'  ✓ Status: {resp["status_name"]}')
            return True
        else:
            print(f'  ✗ Status: {resp["status_name"]} (0x{resp["status_code"]:04x})')
            return False
    except Exception as e:
        print(f'  ✗ Error: {e}')
        return False

def test_print_job(host: str, port: int, pdf_path: str = None) -> bool:
    """Test Print-Job operation with a real or fake PDF."""
    print(f'\n=== Test: Print-Job ===')
    printer_uri = f'ipp://{host}:{port}/ipp/print'

    if pdf_path and os.path.exists(pdf_path):
        with open(pdf_path, 'rb') as f:
            doc_data = f.read()
        print(f'  Using real PDF: {pdf_path} ({len(doc_data)} bytes)')
    else:
        # Minimal valid PDF
        doc_data = b'%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\nxref\n0 4\n0000000000 65535 f\n0000000009 00000 n\n0000000058 00000 n\n0000000115 00000 n\ntrailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n190\n%%EOF'
        print(f'  Using minimal synthetic PDF ({len(doc_data)} bytes)')

    req = build_ipp_request(0x0002, 3, [
        (0x45, 'printer-uri', printer_uri),
        (0x42, 'job-name', 'InkPrint Test Job'),
        (0x42, 'requesting-user-name', 'test-script'),
        (0x49, 'document-format', 'application/pdf'),
    ], doc_data)

    try:
        resp_bytes = send_ipp(host, port, req)
        resp = parse_ipp_response(resp_bytes)

        if resp['ok']:
            print(f'  ✓ Status: {resp["status_name"]}')
            return True
        else:
            print(f'  ✗ Status: {resp["status_name"]} (0x{resp["status_code"]:04x})')
            return False
    except Exception as e:
        print(f'  ✗ Error: {e}')
        return False

def test_unsupported_operation(host: str, port: int) -> bool:
    """Test that unsupported operations return proper error."""
    print(f'\n=== Test: Unsupported Operation (should return error) ===')
    printer_uri = f'ipp://{host}:{port}/ipp/print'

    req = build_ipp_request(0x0012, 99, [  # Purge-Jobs (not supported)
        (0x45, 'printer-uri', printer_uri),
    ])

    try:
        resp_bytes = send_ipp(host, port, req)
        resp = parse_ipp_response(resp_bytes)

        if resp['status_code'] == 0x0501:  # server-error-operation-not-supported
            print(f'  ✓ Correctly returned: {resp["status_name"]}')
            return True
        else:
            print(f'  ✗ Unexpected status: {resp["status_name"]} (0x{resp["status_code"]:04x})')
            return False
    except Exception as e:
        print(f'  ✗ Error: {e}')
        return False

def check_ipptool(host: str, port: int):
    """Run ipptool tests if available (CUPS utility)."""
    import subprocess

    try:
        result = subprocess.run(['which', 'ipptool'], capture_output=True, text=True)
        if result.returncode != 0:
            print('\n  (ipptool not found — skipping CUPS compliance tests)')
            print('  Install with: brew install cups')
            return

        printer_uri = f'ipp://{host}:{port}/ipp/print'
        print(f'\n=== ipptool compliance test ===')

        # Test Get-Printer-Attributes with ipptool
        result = subprocess.run(
            ['ipptool', '-tv', printer_uri, 'get-printer-attributes.test'],
            capture_output=True, text=True, timeout=15
        )
        print(result.stdout[-2000:] if len(result.stdout) > 2000 else result.stdout)
        if result.returncode == 0:
            print('  ✓ ipptool: PASSED')
        else:
            print(f'  ✗ ipptool: FAILED\n{result.stderr}')
    except Exception as e:
        print(f'  ipptool error: {e}')

def main():
    if len(sys.argv) < 2:
        # Default to localhost for local testing
        host = '127.0.0.1'
        port = 631
        print(f'No host specified — testing against localhost:{port}')
        print('Usage: python3 scripts/test_ipp.py <printer_ip> [port]')
    else:
        host = sys.argv[1]
        port = int(sys.argv[2]) if len(sys.argv) > 2 else 631

    pdf_path = sys.argv[3] if len(sys.argv) > 3 else None

    print(f'Testing InkPrint at {host}:{port}')
    print('=' * 50)

    results = []
    results.append(('Get-Printer-Attributes', test_get_printer_attributes(host, port)))
    results.append(('Validate-Job', test_validate_job(host, port)))
    results.append(('Print-Job', test_print_job(host, port, pdf_path)))
    results.append(('Unsupported-Operation', test_unsupported_operation(host, port)))
    check_ipptool(host, port)

    print('\n' + '=' * 50)
    print('Results:')
    passed = sum(1 for _, ok in results if ok)
    for name, ok in results:
        print(f'  {"✓" if ok else "✗"} {name}')
    print(f'\n{passed}/{len(results)} tests passed')

    sys.exit(0 if passed == len(results) else 1)

if __name__ == '__main__':
    main()
