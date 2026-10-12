# Implemented RPC dispatch inventory

This list is generated from handler method names against the pinned layer-231 schema.
Behavior and limitations are documented in README.md. Unsupported API methods return an RPC error.

## account

- `account.checkUsername`
- `account.getNotifySettings`
- `account.registerDevice`
- `account.reorderUsernames`
- `account.toggleUsername`
- `account.unregisterDevice`
- `account.updateProfile`
- `account.updateStatus`
- `account.updateUsername`

## auth

- `auth.bindTempAuthKey` (MTProto session dispatcher)

- `auth.logOut`
- `auth.sendCode`
- `auth.signIn`

## channels

- `channels.checkUsername`
- `channels.createChannel`
- `channels.deactivateAllUsernames`
- `channels.deleteChannel`
- `channels.deleteHistory`
- `channels.deleteMessages`
- `channels.editAdmin`
- `channels.editBanned`
- `channels.editTitle`
- `channels.getAdminLog`
- `channels.getAdminedPublicChannels`
- `channels.getChannelRecommendations`
- `channels.getChannels`
- `channels.getFullChannel`
- `channels.getGroupsForDiscussion`
- `channels.getParticipant`
- `channels.getParticipants`
- `channels.inviteToChannel`
- `channels.joinChannel`
- `channels.leaveChannel`
- `channels.readHistory`
- `channels.readMessageContents`
- `channels.reorderUsernames`
- `channels.setDiscussionGroup`
- `channels.setEmojiStickers`
- `channels.setStickers`
- `channels.toggleJoinRequest`
- `channels.toggleJoinToSend`
- `channels.toggleParticipantsHidden`
- `channels.togglePreHistoryHidden`
- `channels.toggleSignatures`
- `channels.toggleSlowMode`
- `channels.toggleUsername`
- `channels.updateEmojiStatus`
- `channels.updateUsername`

## contacts

- `contacts.getContacts`
- `contacts.resolveUsername`
- `contacts.search`

## fragment

- `fragment.getCollectibleInfo`

## help

- `help.getAppConfig`
- `help.getConfig`
- `help.getNearestDc`
- `help.getPremiumPromo`
- `help.getTermsOfServiceUpdate`

## messages

- `messages.addChatUser`
- `messages.checkChatInvite`
- `messages.createChat`
- `messages.deleteExportedChatInvite`
- `messages.deleteHistory`
- `messages.deleteMessages`
- `messages.deleteRevokedExportedChatInvites`
- `messages.editChatAbout`
- `messages.editExportedChatInvite`
- `messages.editMessage`
- `messages.exportChatInvite`
- `messages.forwardMessages`
- `messages.getAllDrafts`
- `messages.getChatInviteImporters`
- `messages.getChats`
- `messages.getDialogFilters`
- `messages.getDialogs`
- `messages.getExportedChatInvites`
- `messages.getFullChat`
- `messages.getHistory`
- `messages.getPeerDialogs`
- `messages.getPeerSettings`
- `messages.getPinnedDialogs`
- `messages.getReplies`
- `messages.getScheduledHistory`
- `messages.hideAllChatJoinRequests`
- `messages.hideChatJoinRequest`
- `messages.importChatInvite`
- `messages.readHistory`
- `messages.search`
- `messages.sendMedia`
- `messages.sendMessage`
- `messages.setTyping`
- `messages.updatePinnedMessage`
- `messages.uploadMedia`

## payments

- `payments.checkCanSendGift`
- `payments.convertStarGift`
- `payments.getPaymentForm`
- `payments.getPremiumGiftCodeOptions`
- `payments.getResaleStarGifts`
- `payments.getSavedStarGift`
- `payments.getSavedStarGifts`
- `payments.getStarGiftUpgradeAttributes`
- `payments.getStarGiftUpgradePreview`
- `payments.getStarGifts`
- `payments.getStarsGiftOptions`
- `payments.getStarsGiveawayOptions`
- `payments.getStarsStatus`
- `payments.getStarsSubscriptions`
- `payments.getStarsTopupOptions`
- `payments.getStarsTransactions`
- `payments.getStarsTransactionsByID`
- `payments.getUniqueStarGift`
- `payments.saveStarGift`
- `payments.sendStarsForm`
- `payments.toggleStarGiftsPinnedToTop`
- `payments.transferStarGift`
- `payments.updateStarGiftPrice`
- `payments.upgradeStarGift`

## updates

- `updates.getChannelDifference`
- `updates.getDifference`
- `updates.getState`

## upload

- `upload.getFile`
- `upload.saveBigFilePart`
- `upload.saveFilePart`

## users

- `users.getFullUser`
- `users.getUsers`

