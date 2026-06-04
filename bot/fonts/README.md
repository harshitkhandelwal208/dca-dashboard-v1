# Bundled Fonts

The bot renders spreadsheet/report previews on the server with `sharp` and SVG text.
Azure App Service Linux images do not consistently include the desktop fonts used by
the generated SVG/XLSX output, which can turn text such as `Discord 3(TM)` into boxes.

These Noto fonts are bundled from Google Fonts and loaded through `fonts.conf`:

- Noto Sans for Arial, Aptos, and Inter aliases.
- Noto Serif for Georgia and Times aliases.
- Noto Color Emoji for emoji fallback.

The Noto family is distributed under the SIL Open Font License.
