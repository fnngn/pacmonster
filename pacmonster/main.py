from discovery import (
    discover_lxc,
    discover_vm,
    get_host_distro,
    get_lxc_distro
)


def main():

    print()
    print("PACMONSTER")
    print("=" * 50)
    print()

    print("HOST")
    print("-" * 50)
    print(f"Distro: {get_host_distro()}")

    print()

    print("LXC")
    print("-" * 50)

    for lxc in discover_lxc():

        distro = "unknown"

        if lxc["status"] == "running":
            distro = get_lxc_distro(lxc["id"])

        print(
            str(lxc["id"]).rjust(4),
            str(lxc["name"]).ljust(15),
            str(distro).ljust(10),
            lxc["status"]
        )

    print()

    print("VM")
    print("-" * 50)

    for vm in discover_vm():

        print(
            str(vm["id"]).rjust(4),
            str(vm["name"]).ljust(20),
            vm["status"]
        )


if __name__ == "__main__":
    main()
