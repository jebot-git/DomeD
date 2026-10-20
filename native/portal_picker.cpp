// SPDX-License-Identifier: MIT
#include <QtWidgets/QApplication>
#include <QtWidgets/QDialog>
#include <QtWidgets/QDialogButtonBox>
#include <QtWidgets/QLabel>
#include <QtWidgets/QPushButton>
#include <QtWidgets/QListWidget>
#include <QtWidgets/QVBoxLayout>
#include <QtCore/QJsonArray>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <cstdio>
extern "C" int dome_portal_picker(int argc, char **argv) {
    QApplication app(argc, argv);
    app.setApplicationName("DomeD Screen Sharing");
    if (argc != 2) return 2;
    auto config = QJsonDocument::fromJson(argv[1]).object();
    QDialog dialog;
    dialog.setWindowTitle("Share your screen — DomeD");
    auto *layout = new QVBoxLayout(&dialog);
    auto *label = new QLabel(QString("%1 wants to share your screen.\nChoose the monitors it may see. Everything on those monitors will be visible.")
        .arg(config["app_id"].toString().isEmpty() ? "An application" : config["app_id"].toString()));
    label->setTextFormat(Qt::PlainText); label->setWordWrap(true); layout->addWidget(label);
    auto *list = new QListWidget;
    list->setSelectionMode(config["multiple"].toBool() ? QAbstractItemView::ExtendedSelection : QAbstractItemView::SingleSelection);
    for (auto value : config["outputs"].toArray()) {
        auto output = value.toObject();
        auto *item = new QListWidgetItem(QString("%1 — %2 × %3").arg(output["name"].toString())
            .arg(output["width"].toInt()).arg(output["height"].toInt()), list);
        item->setData(Qt::UserRole, output["name"].toString());
    }
    layout->addWidget(list);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
    buttons->button(QDialogButtonBox::Ok)->setText("Share");
    QObject::connect(buttons, &QDialogButtonBox::accepted, [&] { if (!list->selectedItems().isEmpty()) dialog.accept(); });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    layout->addWidget(buttons); dialog.resize(520, 300);
    if (dialog.exec() != QDialog::Accepted) return 1;
    QJsonArray selected;
    for (auto *item : list->selectedItems()) selected.append(item->data(Qt::UserRole).toString());
    auto bytes = QJsonDocument(selected).toJson(QJsonDocument::Compact);
    std::fwrite(bytes.constData(), 1, bytes.size(), stdout);
    std::fputc('\n', stdout); std::fflush(stdout);
    dialog.hide();
    QDialog active;
    active.setWindowTitle("Screen sharing active — DomeD");
    auto *sharingLayout = new QVBoxLayout(&active);
    auto *sharingLabel = new QLabel("Your selected monitors are being shared.");
    sharingLayout->addWidget(sharingLabel);
    auto *stop = new QPushButton("Stop sharing");
    sharingLayout->addWidget(stop);
    QObject::connect(stop, &QPushButton::clicked, &active, &QDialog::accept);
    active.exec();
    return 0;
}
