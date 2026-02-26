.PHONY: install uninstall doctor setup update

install:
	./scripts/install.sh

uninstall:
	./scripts/uninstall.sh

doctor:
	./bin/pis-doctor

setup:
	./bin/pis-setup

update:
	./bin/pis-update
