.PHONY: install install-self uninstall doctor setup update update-agents run run-browser aliases

AGS := cargo run -p ags --

install:
	$(AGS) install

install-self:
	$(AGS) install --link-self

uninstall:
	$(AGS) uninstall

doctor:
	$(AGS) doctor

setup:
	$(AGS) setup

update:
	$(AGS) update

update-agents:
	$(AGS) update-agents

run:
	$(AGS) --agent pi

run-browser:
	$(AGS) --agent pi --browser

aliases:
	$(AGS) create-aliases --mode both
