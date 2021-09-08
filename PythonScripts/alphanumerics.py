# Nemeth alphanumeric unicode converter

from bs4 import BeautifulSoup

def create_unicode_from_html(in_file: str, out_file):
    with open(in_file, encoding='utf8') as _in_stream:
        with open(out_file, 'a', encoding='utf8') as out_stream:
            file_contents = BeautifulSoup(_in_stream, features="html.parser")
            for row in file_contents.find_all('tr'):
                cols = row.find_all('td')
                if len(cols) > 0:
                    generate_char(out_stream, cols[1].get_text(), cols[2].get_text(), cols[3].get_text())

def generate_char(out_stream, hex: str, description: str, braille: str):
    #  - "⬟": [t: "⠫⠸⠢"]              # 0x2B1F (Black pentagon)
    first_part = ' - "{}": [t: "{}"]'.format(chr(int(hex, base=16)), braille)
    out_stream.write('{:32}# 0x{} ({})\n'.format(
            first_part, hex, description))

import os
if os.path.exists("alphanumerics.txt"):
  os.remove("alphanumerics.txt")
path = "C:/Dev/speech-rule-engine/sre-tests/output/nemeth/symbols/"
for filename in os.listdir(path):
   create_unicode_from_html(path+filename, "alphanumerics.txt")