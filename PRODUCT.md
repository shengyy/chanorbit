# Product scope

chanOrbit computes Chan-theory stroke structure from a bar sequence and returns it as JSON. It is a single executable, `chanorbit`, that reads requests on stdin and writes responses on stdout. Callers own data acquisition, price adjustment, multi-period mapping and every decision taken from the output.

## Scope

| Area | Boundary |
|---|---|
| Bars | Columnar exchange-local naive wall-clock bars (`dt`, `high`, `low`, `close`, `vol`) of any period; same-time revisions of the latest bar |
| Inclusion | NewBar merge with direction from the previous two NewBars |
| Fractals | Alternating tops and bottoms with price and confirmation time |
| Strokes | Stroke search, extension, withdrawal and retention of the latest strokes |
| Centres | Canonical stroke-centre partition over the finished strokes |
| Power | MACD(12, 26, 9) and per-interval histogram areas, end values, sample counts and mean volume |
| Candidate | Display-only candidate segment after the last stroke, with its power windows |
| Output | One geometry snapshot per request, carrying only fields a consumer reads, each fact once |

Definitions are in [docs/structure-contract.md](docs/structure-contract.md), the command line in [docs/cli-contract.md](docs/cli-contract.md).

## Non-goals

- Segments (线段) and recursive trend types (走势类型).
- Buy/sell point evaluation and trading signals.
- Backtesting, positions, order handling or any trader.
- Data download, storage or price adjustment.
- A general technical-indicator library.
- Mapping between periods.

The structure is stroke-level: centres are built from strokes, not segments. It does not claim that every stroke or centre satisfies a particular reading of the original theory.

## Intended use and disclaimer

> **Research & Experimental Use Only — Non-commercial intended use**
>
> This project is intended for personal quantitative research, software architecture learning and local experiments. It is not a commercial service and comes without support.
>
> The software provides neither investment advice nor trading signals. Structural output is a computed description of past prices, not a forecast. It is provided **AS IS**, without express or implied warranties. Users bear the risks of incorrect data, incorrect results and any direct or indirect loss from using it. To the fullest extent permitted by applicable law, the authors disclaim liability for such use and losses.
>
> Users must comply with applicable law and the rules of their data providers and brokers.
