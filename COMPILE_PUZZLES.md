# Compile Puzzles

```bash
python3 -m venv venv
. ./venv/bin/activate
pip install --extra-index-url https://pypi.chia.net/simple/ chia_dev_tools==1.2.6
cdv clsp build puzzles/
```
