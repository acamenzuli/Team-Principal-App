"""`python -m voicelab` — start the service."""


def main() -> None:
    from voicelab.server import run

    run()


if __name__ == "__main__":
    main()
