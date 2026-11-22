import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore

PlasmoidItem {
    id: root

    readonly property bool showDownloads: Plasmoid.configuration.cardMode === 0
    readonly property int backendPort: Plasmoid.configuration.backendPort
    readonly property string endpoint: "http://127.0.0.1:" + backendPort + (showDownloads ? "/downloads" : "/system")
    readonly property string title: showDownloads ? i18n("Active downloads") : i18n("System and storage")
    readonly property color ink: "#f1f6fb"
    readonly property color muted: "#a9bed0"
    readonly property color green: "#69e3c4"
    readonly property color blue: "#82b7ff"
    readonly property color amber: "#ffc27f"
    property var feed: ({})
    property var pending: null
    property string statusMessage: ""
    readonly property var dataFeed: feed.data || ({})

    preferredRepresentation: fullRepresentation
    Plasmoid.backgroundHints: PlasmaCore.Types.NoBackground
    toolTipMainText: title
    toolTipSubText: statusMessage || (feed.demo ? i18n("Demo data") : i18n("Local backend connected"))

    function bytes(number) {
        if (number === null || number === undefined)
            return "—";
        if (number >= 1e12)
            return i18n("%1 TB", (number / 1e12).toFixed(2));
        if (number >= 1e9)
            return i18n("%1 GB", (number / 1e9).toFixed(1));
        return i18n("%1 MB", (number / 1e6).toFixed(1));
    }

    function rate(number) {
        if (number === null || number === undefined)
            return "—";
        if (number >= 1e6)
            return i18n("%1 MB/s", (number / 1e6).toFixed(1));
        return i18n("%1 kB/s", (number / 1e3).toFixed(1));
    }

    function eta(seconds) {
        if (!seconds || seconds >= 8640000)
            return "—";
        if (seconds < 60)
            return i18n("<1 min");
        if (seconds < 3600)
            return i18n("%1 min", Math.ceil(seconds / 60));
        if (seconds < 86400)
            return i18n("%1 h", (seconds / 3600).toFixed(1));
        return i18n("%1 d", (seconds / 86400).toFixed(1));
    }

    function stateName(state) {
        const names = {
            downloading: i18n("Downloading"),
            forcedDL: i18n("Downloading"),
            stalledDL: i18n("Waiting for peers"),
            queuedDL: i18n("Queued"),
            pausedDL: i18n("Paused"),
            stoppedDL: i18n("Stopped"),
            metaDL: i18n("Fetching metadata"),
            forcedMetaDL: i18n("Fetching metadata"),
            checkingDL: i18n("Checking"),
            checkingResumeData: i18n("Checking"),
            allocating: i18n("Allocating"),
            moving: i18n("Moving files"),
            error: i18n("Error"),
            missingFiles: i18n("Missing files")
        };
        return names[state] || i18n("Pending");
    }

    function refresh() {
        if (pending)
            return;
        const request = new XMLHttpRequest();
        pending = request;
        request.open("GET", endpoint);
        request.onreadystatechange = () => {
            if (request.readyState !== XMLHttpRequest.DONE || pending !== request)
                return;
            pending = null;
            watchdog.stop();
            if (request.status !== 200) {
                feed = ({});
                statusMessage = i18n("Waiting for the local backend");
                return;
            }
            try {
                feed = JSON.parse(request.responseText);
                statusMessage = feed.message || "";
            } catch (error) {
                feed = ({});
                statusMessage = i18n("The backend returned invalid data");
            }
        };
        watchdog.restart();
        request.send();
    }

    function resetEndpoint() {
        if (pending) {
            const previous = pending;
            pending = null;
            previous.abort();
        }
        watchdog.stop();
        feed = ({});
        statusMessage = i18n("Waiting for the local backend");
        refresh();
    }

    Component.onCompleted: resetEndpoint()
    onEndpointChanged: if (visible)
        resetEndpoint()

    Timer {
        interval: root.showDownloads ? 2500 : 1000
        repeat: true
        running: root.visible
        onTriggered: root.refresh()
    }

    Timer {
        id: watchdog
        interval: 4000
        onTriggered: root.resetEndpoint()
    }

    component TextLabel: Controls.Label {
        color: root.ink
        textFormat: Text.PlainText
        elide: Text.ElideRight
        font.pixelSize: 13
    }

    component Secondary: TextLabel {
        color: root.muted
        font.pixelSize: 11
    }

    component Line: Rectangle {
        Layout.fillWidth: true
        implicitHeight: 1
        color: "#30475b"
    }

    component Meter: Rectangle {
        id: meter
        property real amount: 0
        property color tint: root.green
        Layout.fillWidth: true
        implicitHeight: 6
        radius: height / 2
        color: "#2b4054"

        Rectangle {
            width: parent.width * Math.max(0, Math.min(1, meter.amount))
            height: parent.height
            radius: parent.radius
            color: meter.tint
        }
    }

    component Statistic: Rectangle {
        id: stat
        required property string label
        required property string value
        property color tint: root.ink
        Layout.fillWidth: true
        Layout.preferredWidth: 130
        implicitHeight: 34
        radius: 8
        color: "#203144"
        border.color: "#355069"

        RowLayout {
            anchors.fill: parent
            anchors.margins: 7
            Secondary {
                text: stat.label
                Layout.fillWidth: true
            }
            TextLabel {
                text: stat.value
                color: stat.tint
                font.weight: Font.DemiBold
            }
        }
    }

    component Usage: ColumnLayout {
        id: usage
        required property string label
        required property string details
        required property real amount
        property color tint: root.blue
        Layout.fillWidth: true
        spacing: 4

        RowLayout {
            Layout.fillWidth: true
            TextLabel {
                text: usage.label
                Layout.fillWidth: true
            }
            Secondary {
                text: usage.details
            }
        }
        Meter {
            amount: usage.amount
            tint: usage.tint
        }
    }

    fullRepresentation: Rectangle {
        Layout.minimumWidth: 320
        Layout.minimumHeight: 230
        Layout.preferredWidth: 480
        Layout.preferredHeight: root.showDownloads ? 385 : 365
        color: "#142231"
        border.color: "#355169"
        radius: 15
        gradient: Gradient {
            GradientStop {
                position: 0
                color: "#1c3041"
            }
            GradientStop {
                position: 1
                color: "#142230"
            }
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 15
            spacing: 9

            RowLayout {
                Layout.fillWidth: true
                spacing: 9
                Rectangle {
                    implicitWidth: 3
                    implicitHeight: 24
                    radius: 2
                    color: root.showDownloads ? root.amber : root.blue
                }
                TextLabel {
                    Layout.fillWidth: true
                    text: root.title
                    font.pixelSize: 16
                    font.weight: Font.DemiBold
                }
                Secondary {
                    visible: !!root.feed.demo
                    text: i18n("Demo data")
                    color: root.amber
                }
            }

            Item {
                visible: !root.feed.data
                Layout.fillWidth: true
                Layout.fillHeight: true
                Secondary {
                    anchors.centerIn: parent
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                    text: root.statusMessage
                }
            }

            Loader {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: !!root.feed.data
                active: visible
                sourceComponent: root.showDownloads ? downloadsView : systemView
            }
        }
    }

    Component {
        id: downloadsView

        ColumnLayout {
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                Statistic {
                    label: i18n("Downloading")
                    value: String(root.dataFeed.active ?? "—")
                    tint: root.green
                }
                Statistic {
                    label: i18n("Moving")
                    value: String(root.dataFeed.moving ?? "—")
                    tint: root.amber
                }
                Statistic {
                    label: i18n("Seeding")
                    value: String(root.dataFeed.seeding ?? "—")
                    tint: root.blue
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Secondary {
                    text: i18n("NAME / PROGRESS")
                    Layout.fillWidth: true
                }
                Secondary {
                    text: i18n("DOWN")
                    Layout.preferredWidth: 66
                }
                Secondary {
                    text: i18n("ETA")
                    Layout.preferredWidth: 50
                }
            }
            Line {}

            Flickable {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: width
                contentHeight: downloadRows.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                Controls.ScrollBar.vertical: Controls.ScrollBar {}

                Column {
                    id: downloadRows
                    width: parent.width - 5
                    spacing: 5

                    Secondary {
                        visible: !(root.dataFeed.downloads ?? []).length
                        text: i18n("No active or incomplete downloads")
                    }

                    Repeater {
                        model: root.dataFeed.downloads ?? []

                        Rectangle {
                            id: torrentRow
                            required property var modelData
                            required property int index
                            readonly property bool moving: modelData.state === "moving"
                            readonly property color accent: moving ? root.amber : modelData.down > 0 ? root.green : root.blue
                            width: parent.width
                            height: 61
                            radius: 6
                            color: index % 2 === 0 ? "#203448" : "transparent"

                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 7
                                spacing: 8

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 4
                                    TextLabel {
                                        text: torrentRow.modelData.title
                                        Layout.fillWidth: true
                                    }
                                    Meter {
                                        amount: torrentRow.modelData.progress ?? 0
                                        tint: torrentRow.accent
                                    }
                                    Secondary {
                                        Layout.fillWidth: true
                                        text: {
                                            const row = torrentRow.modelData;
                                            const detail = root.stateName(row.state) + " · " + row.category;
                                            if (row.progress === null)
                                                return i18n("Move progress unavailable") + " · " + row.category;
                                            const percent = (row.progress * 100).toFixed(1);
                                            return (torrentRow.moving ? i18n("≈%1%", percent) : i18n("%1%", percent)) + " · " + detail;
                                        }
                                    }
                                }
                                ColumnLayout {
                                    Layout.preferredWidth: 66
                                    spacing: 4
                                    TextLabel {
                                        text: torrentRow.moving ? "—" : root.rate(torrentRow.modelData.down)
                                        color: torrentRow.modelData.down > 0 ? root.green : root.muted
                                    }
                                    Secondary {
                                        visible: !torrentRow.moving
                                        text: i18np("%1 seed", "%1 seeds", torrentRow.modelData.seeds)
                                    }
                                }
                                Secondary {
                                    Layout.preferredWidth: 43
                                    text: torrentRow.moving ? "—" : root.eta(torrentRow.modelData.eta)
                                }
                            }
                        }
                    }

                    Secondary {
                        topPadding: 7
                        visible: (root.dataFeed.completed ?? []).length > 0
                        text: i18n("RECENTLY FINISHED")
                    }

                    Repeater {
                        model: root.dataFeed.completed ?? []
                        RowLayout {
                            id: completedRow
                            required property var modelData
                            width: parent.width

                            TextLabel {
                                text: completedRow.modelData.title ?? ""
                                Layout.fillWidth: true
                            }
                            Secondary {
                                text: root.bytes(completedRow.modelData.size)
                            }
                            Secondary {
                                text: i18n("Ratio %1", completedRow.modelData.ratio.toFixed(2))
                            }
                        }
                    }
                }
            }

            Line {}
            RowLayout {
                Layout.fillWidth: true
                Secondary {
                    text: i18np("%1 torrent", "%1 torrents", root.dataFeed.total ?? 0) + " · " + i18np("%1 peer connection", "%1 peer connections", root.dataFeed.stats?.total_peer_connections ?? 0)
                }
                Item {
                    Layout.fillWidth: true
                }
                Secondary {
                    visible: root.dataFeed.stats?.free_space_on_disk != null
                    text: i18n("%1 free", root.bytes(root.dataFeed.stats?.free_space_on_disk))
                }
            }
        }
    }

    Component {
        id: systemView

        Flickable {
            id: systemScroller
            clip: true
            contentWidth: width
            contentHeight: systemRows.implicitHeight
            boundsBehavior: Flickable.StopAtBounds
            Controls.ScrollBar.vertical: Controls.ScrollBar {}

            ColumnLayout {
                id: systemRows
                width: systemScroller.width - 5
                spacing: 8

                RowLayout {
                    Layout.fillWidth: true
                    TextLabel {
                        text: root.dataFeed.cpu == null ? i18n("CPU —") : i18n("CPU %1%", root.dataFeed.cpu)
                    }
                    Secondary {
                        visible: root.dataFeed.temperature != null
                        text: i18n("%1 °C", root.dataFeed.temperature)
                    }
                    Item {
                        Layout.fillWidth: true
                    }
                    Secondary {
                        visible: root.dataFeed.uptime != null
                        text: i18n("Up %1 d %2 h", Math.floor((root.dataFeed.uptime ?? 0) / 86400), Math.floor((root.dataFeed.uptime ?? 0) % 86400 / 3600))
                    }
                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: Math.max(1, Math.floor(width / 21))
                    columnSpacing: 4
                    rowSpacing: 4

                    Repeater {
                        model: root.dataFeed.cores ?? []
                        Rectangle {
                            id: core
                            required property var modelData
                            readonly property real percent: modelData.percent ?? 0
                            Layout.fillWidth: true
                            Layout.preferredWidth: 17
                            implicitHeight: 20
                            radius: 3
                            color: "#213a4d"

                            Rectangle {
                                anchors.bottom: parent.bottom
                                width: parent.width
                                height: parent.height * core.percent / 100
                                radius: parent.radius
                                color: core.percent > 85 ? root.amber : root.blue
                            }
                            HoverHandler {
                                id: coreHover
                            }
                            Controls.ToolTip.visible: coreHover.hovered
                            Controls.ToolTip.text: core.modelData.percent == null ? core.modelData.name : i18n("%1 · %2%", core.modelData.name, core.modelData.percent)
                        }
                    }
                }

                Usage {
                    label: i18n("Memory")
                    details: root.bytes(root.dataFeed.memory_used) + " / " + root.bytes(root.dataFeed.memory_total)
                    amount: (root.dataFeed.memory_used ?? 0) / (root.dataFeed.memory_total || 1)
                    tint: root.green
                }

                Usage {
                    visible: (root.dataFeed.swap_total ?? 0) > 0
                    label: i18n("Swap")
                    details: root.bytes(root.dataFeed.swap_used) + " / " + root.bytes(root.dataFeed.swap_total)
                    amount: (root.dataFeed.swap_used ?? 0) / (root.dataFeed.swap_total || 1)
                }

                Line {
                    visible: (root.dataFeed.disks ?? []).length > 0
                }

                Repeater {
                    model: root.dataFeed.disks ?? []
                    Usage {
                        id: disk
                        required property var modelData
                        label: disk.modelData.name
                        details: disk.modelData.total == null ? i18n("Mount unavailable") : i18n("%1 free of %2", root.bytes(disk.modelData.free), root.bytes(disk.modelData.total))
                        amount: (disk.modelData.used ?? 0) / (disk.modelData.total || 1)
                        tint: disk.modelData.free / disk.modelData.total < 0.1 ? root.amber : root.blue
                    }
                }

                Line {
                    visible: (root.dataFeed.drive_rates ?? []).length > 0
                }

                Repeater {
                    model: root.dataFeed.drive_rates ?? []
                    RowLayout {
                        id: drive
                        required property var modelData
                        Layout.fillWidth: true
                        TextLabel {
                            text: drive.modelData.name
                            Layout.fillWidth: true
                        }
                        Secondary {
                            text: i18n("Read %1 · Write %2", root.rate(drive.modelData.read), root.rate(drive.modelData.write))
                        }
                    }
                }
            }
        }
    }
}
