import { Button, Input, Modal, Space, Typography, type ButtonProps } from "antd";
import type { ReactNode } from "react";
import { useState } from "react";

interface ConfirmActionButtonProps {
  title: string;
  description?: ReactNode;
  confirmText?: string;
  okText?: string;
  loading?: boolean;
  buttonProps?: ButtonProps;
  children: ReactNode;
  onConfirm: () => void;
}

export function ConfirmActionButton({
  title,
  description,
  confirmText = "确认",
  okText = "确认执行",
  loading,
  buttonProps,
  children,
  onConfirm
}: ConfirmActionButtonProps) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  const matched = value.trim() === confirmText;

  const close = () => {
    setOpen(false);
    setValue("");
  };

  return (
    <>
      <Button
        {...buttonProps}
        loading={loading}
        onClick={(event) => {
          buttonProps?.onClick?.(event);
          setOpen(true);
        }}
      >
        {children}
      </Button>
      <Modal
        title={title}
        open={open}
        okText={okText}
        cancelText="取消"
        onCancel={close}
        onOk={() => {
          onConfirm();
          close();
        }}
        okButtonProps={{
          danger: buttonProps?.danger,
          disabled: !matched,
          loading
        }}
      >
        <Space direction="vertical" size={12} className="settings-stack">
          {description ? (
            <Typography.Text type="secondary">{description}</Typography.Text>
          ) : null}
          <Typography.Text>
            输入 <Typography.Text code>{confirmText}</Typography.Text> 后继续
          </Typography.Text>
          <Input
            autoFocus
            value={value}
            onChange={(event) => setValue(event.target.value)}
          />
        </Space>
      </Modal>
    </>
  );
}
