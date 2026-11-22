import QtQuick
import QtQuick.Controls as Controls
import org.kde.kirigami as Kirigami

Kirigami.FormLayout {
    property alias cfg_cardMode: cardMode.currentIndex
    property alias cfg_backendPort: backendPort.value

    Controls.ComboBox {
        id: cardMode
        Kirigami.FormData.label: i18n("Card")
        model: [i18n("Active downloads"), i18n("System and storage")]
    }

    Controls.SpinBox {
        id: backendPort
        Kirigami.FormData.label: i18n("Local backend port")
        editable: true
        from: 1
        to: 65535
    }
}
