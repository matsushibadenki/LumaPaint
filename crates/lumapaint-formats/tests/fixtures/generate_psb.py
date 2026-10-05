"""Convert our own small RGB8 PSD fixtures to PSB, independently of Rust reader.
Run from any directory with Python 3; no external packages or artwork required.
"""
from pathlib import Path
from struct import pack, unpack_from

ROOT = Path(__file__).resolve().parent

class Reader:
    def __init__(self, data):
        self.data, self.at = data, 0
    def take(self, size):
        value = self.data[self.at:self.at + size]
        assert len(value) == size
        self.at += size
        return value
    def uint(self, size):
        return int.from_bytes(self.take(size), 'big')
    def section(self):
        return self.take(self.uint(4))

def channel(data, height):
    if data[:2] != b'\0\1':
        return data
    counts = unpack_from('>' + 'H' * height, data, 2)
    return data[:2] + b''.join(pack('>I', n) for n in counts) + data[2 + height * 2:]

def convert(source):
    r = Reader(source)
    header = bytearray(r.take(26))
    assert header[:6] == b'8BPS\0\1'
    header[4:6] = b'\0\2'
    channels, height = unpack_from('>HI', header, 12)
    color, resources = r.section(), r.section()
    outer = Reader(r.section())
    info = Reader(outer.section())
    count_bytes = info.take(2)
    count = abs(int.from_bytes(count_bytes, 'big', signed=True))
    records = []
    for _ in range(count):
        rect = info.take(16)
        top, left, bottom, right = unpack_from('>iiii', rect)
        n = info.uint(2)
        counts = [(info.take(2), info.uint(4)) for _ in range(n)]
        blend = info.take(12)
        extra = info.section()
        records.append((rect, bottom - top, counts, blend, extra))
    result, payloads = bytearray(count_bytes), []
    for rect, rows, counts, blend, extra in records:
        result.extend(rect + pack('>H', len(counts)))
        for cid, size in counts:
            data = channel(info.take(size), rows)
            result.extend(cid + pack('>Q', len(data)))
            payloads.append(data)
        result.extend(blend + pack('>I', len(extra)) + extra)
    result.extend(b''.join(payloads))
    if len(result) % 2:
        result.append(0)
    assert info.take(len(info.data) - info.at) in (b'', b'\0')
    layer_data = pack('>Q', len(result)) + result + outer.take(len(outer.data) - outer.at)
    merged = channel(r.take(len(r.data) - r.at), channels * height)
    return bytes(header) + pack('>I', len(color)) + color + pack('>I', len(resources)) + resources + pack('>Q', len(layer_data)) + layer_data + merged

if __name__ == '__main__':
    for suffix in ['', '-rle', '-zip', '-prediction']:
        source = ROOT / ('lp-psd-layers' + suffix + '.psd')
        (ROOT / ('lp-psb-layers' + suffix + '.psb')).write_bytes(convert(source.read_bytes()))
